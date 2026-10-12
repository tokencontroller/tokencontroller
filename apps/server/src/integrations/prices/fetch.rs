//! Fetches both price sources and merges them, carrying on when one fails.

use super::{ModelPrice, PriceError, PriceSource, merge, parse_litellm, parse_models_dev};

/// `LiteLLM`'s price file.
pub const LITELLM_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

/// The `models.dev` catalogue.
pub const MODELS_DEV_URL: &str = "https://models.dev/api.json";

/// A fetched price table.
#[derive(Debug)]
pub struct Fetched {
    /// The merged price table, sorted.
    pub prices: Vec<ModelPrice>,
    /// Why each source that failed could not be read; each error names its
    /// source. Empty when both were read.
    pub failed_sources: Vec<PriceError>,
}

/// Fetches `LiteLLM` and `models.dev` at once and merges their prices. If one
/// source fails, the price table comes from the other and the failure is in
/// [`Fetched::failed_sources`].
///
/// # Errors
///
/// [`PriceError::AllSourcesFailed`] only if neither source can be read.
pub async fn fetch(
    client: &reqwest::Client,
    litellm_url: &str,
    models_dev_url: &str,
) -> Result<Fetched, PriceError> {
    let (litellm, models_dev) = tokio::join!(
        download(client, litellm_url, PriceSource::LiteLlm),
        download(client, models_dev_url, PriceSource::ModelsDev),
    );
    let litellm = litellm.and_then(|body| parse_litellm(&body));
    let models_dev = models_dev.and_then(|body| parse_models_dev(&body));
    match (litellm, models_dev) {
        (Ok(litellm), Ok(models_dev)) => Ok(Fetched {
            prices: merge(litellm, models_dev),
            failed_sources: Vec::new(),
        }),
        (Ok(litellm), Err(failure)) => Ok(Fetched {
            prices: merge(litellm, Vec::new()),
            failed_sources: vec![failure],
        }),
        (Err(failure), Ok(models_dev)) => Ok(Fetched {
            prices: merge(Vec::new(), models_dev),
            failed_sources: vec![failure],
        }),
        (Err(litellm), Err(models_dev)) => {
            Err(PriceError::AllSourcesFailed(vec![litellm, models_dev]))
        }
    }
}

/// The body of a successful `GET`.
async fn download(
    client: &reqwest::Client,
    url: &str,
    source: PriceSource,
) -> Result<String, PriceError> {
    let failed = |error| PriceError::Fetch(source, error);
    client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(failed)?
        .text()
        .await
        .map_err(failed)
}

#[cfg(test)]
mod tests {
    // Fixtures: cut from LiteLLM's model_prices_and_context_window.json
    // (https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json,
    // fields as its `sample_spec` entry documents them) and models.dev's
    // api.json (https://models.dev/api.json, schema in the "Schema
    // Reference" of https://github.com/sst/models.dev), both fetched
    // 2026-10-12, served as raw files the way GitHub and models.dev serve them.
    use super::*;
    use crate::integrations::http::client;
    use crate::integrations::prices::lookup;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const LITELLM: &str = include_str!("../../../tests/fixtures/prices/litellm.json");
    const MODELS_DEV: &str = include_str!("../../../tests/fixtures/prices/models_dev.json");

    async fn serve(server: &MockServer, route: &str, response: ResponseTemplate) {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(response)
            .expect(1)
            .mount(server)
            .await;
    }

    fn json(body: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_raw(body.to_owned(), "application/json")
    }

    fn urls(server: &MockServer) -> (String, String) {
        (
            format!("{}/litellm.json", server.uri()),
            format!("{}/api.json", server.uri()),
        )
    }

    #[tokio::test]
    async fn both_sources_merge_into_one_table() {
        let server = MockServer::start().await;
        serve(&server, "/litellm.json", json(LITELLM)).await;
        serve(&server, "/api.json", json(MODELS_DEV)).await;
        let (litellm, models_dev) = urls(&server);

        let fetched = fetch(&client().unwrap(), &litellm, &models_dev)
            .await
            .unwrap();

        assert!(fetched.failed_sources.is_empty());
        assert_eq!(fetched.prices.len(), 14);
        assert!(lookup(&fetched.prices, "gpt-5.3-codex-spark").is_some());
        let sonnet = lookup(&fetched.prices, "claude-sonnet-4-5-20250929").unwrap();
        assert_eq!(sonnet.provider.as_deref(), Some("anthropic"));
    }

    #[tokio::test]
    async fn a_failing_source_leaves_the_other() {
        let server = MockServer::start().await;
        serve(&server, "/litellm.json", json(LITELLM)).await;
        serve(&server, "/api.json", ResponseTemplate::new(503)).await;
        let (litellm, models_dev) = urls(&server);

        let fetched = fetch(&client().unwrap(), &litellm, &models_dev)
            .await
            .unwrap();

        assert_eq!(fetched.prices, parse_litellm(LITELLM).unwrap());
        assert_eq!(fetched.failed_sources.len(), 1);
        assert!(matches!(
            fetched.failed_sources[0],
            PriceError::Fetch(PriceSource::ModelsDev, _)
        ));
        assert!(fetched.failed_sources[0].to_string().contains("503"));
    }

    #[tokio::test]
    async fn an_unreadable_file_counts_as_a_failed_source() {
        let server = MockServer::start().await;
        serve(
            &server,
            "/litellm.json",
            ResponseTemplate::new(200).set_body_string("<html>404: Not Found</html>"),
        )
        .await;
        serve(&server, "/api.json", json(MODELS_DEV)).await;
        let (litellm, models_dev) = urls(&server);

        let fetched = fetch(&client().unwrap(), &litellm, &models_dev)
            .await
            .unwrap();

        assert_eq!(fetched.prices, parse_models_dev(MODELS_DEV).unwrap());
        assert!(matches!(
            fetched.failed_sources.as_slice(),
            [PriceError::BadFile(PriceSource::LiteLlm, _)]
        ));
    }

    #[tokio::test]
    async fn it_fails_only_when_both_sources_fail() {
        let server = MockServer::start().await;
        serve(&server, "/litellm.json", ResponseTemplate::new(500)).await;
        serve(&server, "/api.json", json("{}")).await;
        let (litellm, models_dev) = urls(&server);

        let error = fetch(&client().unwrap(), &litellm, &models_dev)
            .await
            .unwrap_err();

        let PriceError::AllSourcesFailed(failures) = &error else {
            panic!("expected both sources to fail, got {error}");
        };
        assert!(matches!(
            failures.as_slice(),
            [
                PriceError::Fetch(PriceSource::LiteLlm, _),
                PriceError::NoPrices(PriceSource::ModelsDev)
            ]
        ));
        let message = error.to_string();
        assert!(message.contains("LiteLLM"));
        assert!(message.contains("models.dev"));
    }

    #[tokio::test]
    async fn an_unreachable_host_is_a_failed_source() {
        let server = MockServer::start().await;
        serve(&server, "/api.json", json(MODELS_DEV)).await;
        let (_, models_dev) = urls(&server);

        let fetched = fetch(
            &client().unwrap(),
            "http://127.0.0.1:9/litellm.json",
            &models_dev,
        )
        .await
        .unwrap();

        assert!(!fetched.prices.is_empty());
        assert!(matches!(
            fetched.failed_sources.as_slice(),
            [PriceError::Fetch(PriceSource::LiteLlm, _)]
        ));
    }

    #[test]
    fn the_real_sources_are_https() {
        assert!(LITELLM_URL.starts_with("https://raw.githubusercontent.com/BerriAI/litellm/"));
        assert_eq!(MODELS_DEV_URL, "https://models.dev/api.json");
    }
}
