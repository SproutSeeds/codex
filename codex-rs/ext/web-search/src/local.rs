use std::env;

use codex_api::SearchRequest;
use codex_login::default_client::build_reqwest_client;
use serde_json::Value;
use url::Url;

pub(crate) const CODEX_LOCAL_SEARCH_URL_ENV: &str = "CODEX_WEB_SEARCH_LOCAL_URL";
pub(crate) const UMBRA_SEARCHD_URL_ENV: &str = "UMBRA_SEARCHD_URL";
pub(crate) const DEFAULT_LOCAL_SEARCH_URL: &str = "http://127.0.0.1:8765";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LocalSearchBackend {
    base_url: String,
}

impl LocalSearchBackend {
    pub(crate) fn from_env() -> Self {
        let base_url = env::var(CODEX_LOCAL_SEARCH_URL_ENV)
            .or_else(|_| env::var(UMBRA_SEARCHD_URL_ENV))
            .unwrap_or_else(|_| DEFAULT_LOCAL_SEARCH_URL.to_string());
        Self::new(base_url)
    }

    pub(crate) fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    #[cfg(test)]
    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    pub(crate) async fn search(&self, request: &SearchRequest) -> Result<String, String> {
        let endpoint = self.search_endpoint()?;
        let response = build_reqwest_client()
            .post(endpoint)
            .json(request)
            .send()
            .await
            .map_err(|err| format!("local web search broker request failed: {err}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|err| format!("local web search broker response read failed: {err}"))?;
        if !status.is_success() {
            return Err(format!(
                "local web search broker returned {status}: {}",
                body.chars().take(500).collect::<String>()
            ));
        }
        output_from_response_body(&body)
    }

    fn search_endpoint(&self) -> Result<Url, String> {
        search_endpoint(&self.base_url)
    }
}

fn search_endpoint(base_url: &str) -> Result<Url, String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    let endpoint = if trimmed.ends_with("/search") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/search")
    };
    Url::parse(&endpoint).map_err(|err| format!("invalid local web search broker URL: {err}"))
}

fn output_from_response_body(body: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|err| format!("local web search broker returned invalid JSON: {err}"))?;
    value
        .get("output")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| "local web search broker JSON missing string field `output`".to_string())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::LocalSearchBackend;
    use super::output_from_response_body;
    use super::search_endpoint;

    #[test]
    fn local_search_endpoint_defaults_to_search_path() {
        assert_eq!(
            search_endpoint("http://127.0.0.1:8765")
                .expect("parse endpoint")
                .as_str(),
            "http://127.0.0.1:8765/search"
        );
        assert_eq!(
            search_endpoint("http://127.0.0.1:8765/search")
                .expect("parse endpoint")
                .as_str(),
            "http://127.0.0.1:8765/search"
        );
    }

    #[test]
    fn local_search_response_body_extracts_plain_output() {
        assert_eq!(
            output_from_response_body(r#"{"encrypted_output":null,"output":"search result"}"#)
                .expect("extract output"),
            "search result"
        );
    }

    #[test]
    fn local_search_backend_preserves_configured_base_url() {
        let backend = LocalSearchBackend::new("http://localhost:7777");
        assert_eq!(backend.base_url(), "http://localhost:7777");
    }
}
