//! The few Stripe API calls billing makes. Prices are found by lookup key,
//! never by ID, because IDs differ between test and live mode.

use std::sync::OnceLock;
use std::time::Duration;

use anyhow::Context as _;
use serde::Deserialize;
use uuid::Uuid;

use super::StripeConfig;
use crate::error::ApiError;

const API: &str = "https://api.stripe.com/v1";

/// ISO 3166 codes of the EU member states. Customers there pay in EUR.
const EU: [&str; 27] = [
    "AT", "BE", "BG", "CY", "CZ", "DE", "DK", "EE", "ES", "FI", "FR", "GR", "HR", "HU", "IE", "IT",
    "LT", "LU", "LV", "MT", "NL", "PL", "PT", "RO", "SE", "SI", "SK",
];

/// Whether a billing country is in the EU.
pub(super) fn is_eu(country: &str) -> bool {
    EU.contains(&country)
}

/// What a Checkout Session needs.
pub(super) struct CheckoutParams<'a> {
    pub public_url: &'a str,
    pub organization_id: Uuid,
    pub lookup_key: &'static str,
    pub eur: bool,
    /// The organization's Stripe customer, once it has one.
    pub customer: Option<&'a str>,
    /// The controller's address, for a first checkout.
    pub email: &'a str,
}

#[derive(Debug, Deserialize)]
struct Url {
    url: String,
}

#[derive(Debug, Deserialize)]
struct List<T> {
    data: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct Price {
    id: String,
}

/// Creates a Checkout Session for the Team plan and returns its URL.
///
/// The parameters follow the private skill on Stripe billing: card only,
/// no converted prices, address and tax ID required where supported, our
/// branding, and no trial of any kind.
pub(super) async fn checkout_session(
    stripe: &StripeConfig,
    params: &CheckoutParams<'_>,
) -> Result<String, ApiError> {
    let client = client()?;
    let price = price_id(client, stripe, params.lookup_key).await?;
    let organization_id = params.organization_id.to_string();
    let success_url = format!("{}/billing?checkout=success", params.public_url);
    let cancel_url = format!("{}/billing", params.public_url);
    let mut form: Vec<(&str, &str)> = vec![
        ("mode", "subscription"),
        ("line_items[0][price]", price.as_str()),
        ("line_items[0][quantity]", "1"),
        ("allowed_payment_method_types[]", "card"),
        ("adaptive_pricing[enabled]", "false"),
        ("billing_address_collection", "required"),
        ("tax_id_collection[enabled]", "true"),
        ("tax_id_collection[required]", "if_supported"),
        ("branding_settings[display_name]", "Token Controller"),
        ("branding_settings[button_color]", "#2563CF"),
        ("branding_settings[font_family]", "inter"),
        ("client_reference_id", organization_id.as_str()),
        ("metadata[organization_id]", organization_id.as_str()),
        (
            "subscription_data[metadata][organization_id]",
            organization_id.as_str(),
        ),
        ("success_url", success_url.as_str()),
        ("cancel_url", cancel_url.as_str()),
    ];
    if params.eur {
        form.push(("currency", "eur"));
    }
    match params.customer {
        Some(customer) => {
            form.push(("customer", customer));
            form.push(("customer_update[name]", "auto"));
            form.push(("customer_update[address]", "auto"));
        }
        None => form.push(("customer_email", params.email)),
    }
    let session: Url = post(client, stripe, "/checkout/sessions", &form).await?;
    Ok(session.url)
}

/// Creates a customer portal session with the account's default
/// configuration and returns its URL.
pub(super) async fn portal_session(
    stripe: &StripeConfig,
    public_url: &str,
    customer: &str,
) -> Result<String, ApiError> {
    let client = client()?;
    let return_url = format!("{public_url}/billing");
    let session: Url = post(
        client,
        stripe,
        "/billing_portal/sessions",
        &[("customer", customer), ("return_url", return_url.as_str())],
    )
    .await?;
    Ok(session.url)
}

async fn price_id(
    client: &reqwest::Client,
    stripe: &StripeConfig,
    lookup_key: &str,
) -> Result<String, ApiError> {
    let prices: List<Price> = client
        .get(format!("{API}/prices"))
        .bearer_auth(&stripe.secret_key)
        .query(&[
            ("lookup_keys[]", lookup_key),
            ("active", "true"),
            ("limit", "1"),
        ])
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .context("Stripe did not list the prices")?
        .json()
        .await
        .context("Stripe sent prices we could not read")?;
    prices
        .data
        .into_iter()
        .next()
        .map(|price| price.id)
        .ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "Stripe has no active price with lookup key {lookup_key}"
            ))
        })
}

async fn post<T: for<'de> Deserialize<'de>>(
    client: &reqwest::Client,
    stripe: &StripeConfig,
    path: &str,
    form: &[(&str, &str)],
) -> Result<T, ApiError> {
    let response = client
        .post(format!("{API}{path}"))
        .bearer_auth(&stripe.secret_key)
        .form(form)
        .send()
        .await
        .with_context(|| format!("could not reach Stripe for {path}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(ApiError::Internal(anyhow::anyhow!(
            "Stripe refused {path} with {status}: {body}"
        )));
    }
    Ok(response
        .json()
        .await
        .with_context(|| format!("Stripe answered {path} with something we could not read"))?)
}

/// The HTTPS client for Stripe, made once, with TLS from `ring` like every
/// other outside call the service makes.
fn client() -> Result<&'static reqwest::Client, ApiError> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    // A second install is harmless: the service uses ring everywhere.
    let _already_installed = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("tokencontroller/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("could not build the HTTP client")?;
    Ok(CLIENT.get_or_init(|| client))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eu_customers_pay_in_euros() {
        for country in ["AT", "DE", "GR", "IE"] {
            assert!(is_eu(country), "{country}");
        }
        for country in ["US", "CH", "GB", "NO", "EL", "at"] {
            assert!(!is_eu(country), "{country}");
        }
        assert_eq!(EU.len(), 27);
    }

    #[test]
    fn the_client_builds_without_system_certificates() {
        assert!(client().is_ok());
    }
}
