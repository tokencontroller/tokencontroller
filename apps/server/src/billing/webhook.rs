//! Stripe's webhook: keeps each organization's subscription state.
//!
//! Subscribe the endpoint to exactly these events:
//! `checkout.session.completed`, `customer.subscription.created`,
//! `customer.subscription.updated`, `customer.subscription.deleted`.
//!
//! Events can arrive late and out of order, so each subscription event is
//! applied only if it is newer than the last one applied.

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use hmac::{Hmac, KeyInit as _, Mac as _};
use serde::Deserialize;
use sha2::Sha256;
use time::OffsetDateTime;
use uuid::Uuid;

use super::stripe_config;
use crate::AppState;
use crate::error::ApiError;

/// How old a signed event may be, in seconds (Stripe's own default).
const TOLERANCE_SECONDS: i64 = 300;

#[derive(Debug, Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
    /// When Stripe created the event, in Unix seconds.
    created: i64,
    data: EventData,
}

#[derive(Debug, Deserialize)]
struct EventData {
    object: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct CheckoutSession {
    client_reference_id: Option<String>,
    customer: Option<String>,
    subscription: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Subscription {
    id: String,
    customer: String,
    status: String,
    #[serde(default)]
    cancel_at_period_end: bool,
    /// Older API versions keep the period end here; newer ones on the item.
    current_period_end: Option<i64>,
    #[serde(default)]
    metadata: std::collections::BTreeMap<String, String>,
    items: Items,
}

#[derive(Debug, Deserialize)]
struct Items {
    data: Vec<Item>,
}

#[derive(Debug, Deserialize)]
struct Item {
    current_period_end: Option<i64>,
    price: ItemPrice,
}

#[derive(Debug, Deserialize)]
struct ItemPrice {
    recurring: Option<Recurring>,
}

#[derive(Debug, Deserialize)]
struct Recurring {
    interval: String,
}

/// Stripe's webhook. Answers 400 to anything not signed with the webhook
/// secret.
#[utoipa::path(
    post,
    path = "/billing/stripe-webhook",
    tag = "billing",
    request_body(content = String, content_type = "application/json", description = "A Stripe event"),
    responses(
        (status = NO_CONTENT, description = "Received"),
        (status = BAD_REQUEST, description = "Not signed by Stripe", body = crate::error::ErrorBody),
        (status = NOT_FOUND, description = "Billing is off on this instance", body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn receive(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let stripe = stripe_config(&state)?;
    let signature = headers
        .get("stripe-signature")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    verify(
        signature,
        &body,
        &stripe.webhook_secret,
        OffsetDateTime::now_utc().unix_timestamp(),
    )?;
    let event: Event = serde_json::from_slice(&body)
        .map_err(|error| ApiError::BadRequest(format!("not a Stripe event: {error}")))?;
    apply(&state, event).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Checks a `Stripe-Signature` header: `t=<unix seconds>,v1=<hex>`, where
/// the hex is HMAC-SHA256 of `<t>.<body>` with the webhook secret.
fn verify(header: &str, body: &[u8], secret: &str, now: i64) -> Result<(), ApiError> {
    let refuse = |why: &str| ApiError::BadRequest(format!("Stripe signature: {why}"));
    let mut timestamp = None;
    let mut signatures = Vec::new();
    for part in header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", value)) => timestamp = value.parse::<i64>().ok(),
            Some(("v1", value)) => signatures.push(value),
            _ => {}
        }
    }
    let timestamp = timestamp.ok_or_else(|| refuse("no timestamp"))?;
    if now.abs_diff(timestamp) > TOLERANCE_SECONDS.unsigned_abs() {
        return Err(refuse("too old"));
    }
    let signed = |signature: &str| {
        let Ok(expected) = hex::decode(signature) else {
            return false;
        };
        let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
            return false;
        };
        mac.update(timestamp.to_string().as_bytes());
        mac.update(b".");
        mac.update(body);
        mac.verify_slice(&expected).is_ok()
    };
    if signatures.into_iter().any(signed) {
        Ok(())
    } else {
        Err(refuse("does not match"))
    }
}

async fn apply(state: &AppState, event: Event) -> Result<(), ApiError> {
    let at = OffsetDateTime::from_unix_timestamp(event.created)
        .map_err(|error| ApiError::BadRequest(format!("event time: {error}")))?;
    match event.kind.as_str() {
        "checkout.session.completed" => {
            let session: CheckoutSession = parse(event.data.object)?;
            checkout_completed(state, session).await
        }
        "customer.subscription.created"
        | "customer.subscription.updated"
        | "customer.subscription.deleted" => {
            let subscription: Subscription = parse(event.data.object)?;
            subscription_changed(state, subscription, at).await
        }
        other => {
            tracing::info!(event = other, "ignored a Stripe event billing does not use");
            Ok(())
        }
    }
}

fn parse<T: for<'de> Deserialize<'de>>(object: serde_json::Value) -> Result<T, ApiError> {
    serde_json::from_value(object)
        .map_err(|error| ApiError::BadRequest(format!("unexpected Stripe object: {error}")))
}

async fn checkout_completed(state: &AppState, session: CheckoutSession) -> Result<(), ApiError> {
    let (Some(organization_id), Some(customer)) = (
        session
            .client_reference_id
            .as_deref()
            .and_then(|id| Uuid::parse_str(id).ok()),
        session.customer,
    ) else {
        tracing::warn!("a completed checkout without organization or customer");
        return Ok(());
    };
    sqlx::query!(
        r#"INSERT INTO billing_subscriptions (organization_id, stripe_customer_id, stripe_subscription_id)
           SELECT $1, $2, $3 WHERE EXISTS (SELECT 1 FROM organizations WHERE id = $1)
           ON CONFLICT (organization_id) DO UPDATE SET
             stripe_customer_id = EXCLUDED.stripe_customer_id,
             stripe_subscription_id = coalesce(EXCLUDED.stripe_subscription_id, billing_subscriptions.stripe_subscription_id),
             updated_at = now()"#,
        organization_id,
        customer,
        session.subscription
    )
    .execute(&state.db)
    .await?;
    tracing::info!(%organization_id, "Team plan checkout completed");
    Ok(())
}

async fn subscription_changed(
    state: &AppState,
    subscription: Subscription,
    at: OffsetDateTime,
) -> Result<(), ApiError> {
    let from_metadata = subscription
        .metadata
        .get("organization_id")
        .and_then(|id| Uuid::parse_str(id).ok());
    let organization_id =
        match from_metadata {
            Some(id) => Some(id),
            None => sqlx::query_scalar!(
                "SELECT organization_id FROM billing_subscriptions WHERE stripe_customer_id = $1",
                subscription.customer
            )
            .fetch_optional(&state.db)
            .await?,
        };
    let Some(organization_id) = organization_id else {
        tracing::warn!(subscription = %subscription.id, "a subscription that belongs to no organization");
        return Ok(());
    };
    let item = subscription.items.data.first();
    let interval = item
        .and_then(|item| item.price.recurring.as_ref())
        .map(|recurring| recurring.interval.as_str())
        .filter(|interval| matches!(*interval, "month" | "year"));
    let period_end = subscription
        .current_period_end
        .or_else(|| item.and_then(|item| item.current_period_end))
        .and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds).ok());
    sqlx::query!(
        r#"INSERT INTO billing_subscriptions
             (organization_id, stripe_customer_id, stripe_subscription_id, status, billing_interval,
              current_period_end, cancel_at_period_end, last_event_at)
           SELECT $1, $2, $3, $4, $5, $6, $7, $8 WHERE EXISTS (SELECT 1 FROM organizations WHERE id = $1)
           ON CONFLICT (organization_id) DO UPDATE SET
             stripe_customer_id = EXCLUDED.stripe_customer_id,
             stripe_subscription_id = EXCLUDED.stripe_subscription_id,
             status = EXCLUDED.status,
             billing_interval = EXCLUDED.billing_interval,
             current_period_end = EXCLUDED.current_period_end,
             cancel_at_period_end = EXCLUDED.cancel_at_period_end,
             last_event_at = EXCLUDED.last_event_at,
             updated_at = now()
           WHERE billing_subscriptions.last_event_at <= EXCLUDED.last_event_at"#,
        organization_id,
        subscription.customer,
        subscription.id,
        subscription.status,
        interval,
        period_end,
        subscription.cancel_at_period_end,
        at
    )
    .execute(&state.db)
    .await?;
    tracing::info!(%organization_id, status = %subscription.status, "Team plan subscription changed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "whsec_test_secret";

    fn sign(body: &[u8], timestamp: i64, secret: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(format!("{timestamp}.").as_bytes());
        mac.update(body);
        format!(
            "t={timestamp},v1={}",
            hex::encode(mac.finalize().into_bytes())
        )
    }

    #[test]
    fn a_correct_signature_passes() {
        let body = br#"{"type":"x"}"#;
        let header = sign(body, 1_000, SECRET);
        assert!(verify(&header, body, SECRET, 1_000).is_ok());
        assert!(verify(&header, body, SECRET, 1_300).is_ok());
    }

    #[test]
    fn a_wrong_secret_body_or_old_event_fails() {
        let body = br#"{"type":"x"}"#;
        let header = sign(body, 1_000, SECRET);
        assert!(verify(&header, body, "whsec_other", 1_000).is_err());
        assert!(verify(&header, b"{}", SECRET, 1_000).is_err());
        assert!(verify(&header, body, SECRET, 1_301).is_err());
        assert!(verify(&header, body, SECRET, 699).is_err());
        assert!(verify("", body, SECRET, 1_000).is_err());
        assert!(verify("t=1000,v1=zz", body, SECRET, 1_000).is_err());
    }

    #[test]
    fn any_of_several_signatures_may_match() {
        let body = b"{}";
        let good = sign(body, 5, SECRET);
        let header = format!("t=5,v1=00ff,{}", good.trim_start_matches("t=5,"));
        assert!(verify(&header, body, SECRET, 5).is_ok());
    }

    #[test]
    fn subscriptions_parse_from_both_api_versions() {
        let old: Subscription = serde_json::from_value(serde_json::json!({
            "id": "sub_1", "customer": "cus_1", "status": "active",
            "current_period_end": 1_700_000_000,
            "items": {"data": [{"price": {"recurring": {"interval": "year"}}}]}
        }))
        .unwrap();
        assert_eq!(old.current_period_end, Some(1_700_000_000));
        let new: Subscription = serde_json::from_value(serde_json::json!({
            "id": "sub_1", "customer": "cus_1", "status": "active", "cancel_at_period_end": true,
            "metadata": {"organization_id": "00000000-0000-0000-0000-000000000001"},
            "items": {"data": [{"current_period_end": 1_700_000_001, "price": {"recurring": {"interval": "month"}}}]}
        }))
        .unwrap();
        assert!(new.cancel_at_period_end);
        assert_eq!(
            new.items.data.first().unwrap().current_period_end,
            Some(1_700_000_001)
        );
    }
}
