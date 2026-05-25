use std::collections::HashMap;
use std::ffi::OsString;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::OnceLock;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::routing::post;
use codex_config::types::OAuthCredentialsStoreMode;
use codex_rmcp_client::perform_oauth_login_return_url;
use pretty_assertions::assert_eq;
use reqwest::Url;
use serde_json::json;
use serial_test::serial;
use tempfile::TempDir;
use tokio::sync::Mutex as AsyncMutex;
use tokio::task::JoinHandle;

struct TempCodexHome {
    _guard: MutexGuard<'static, ()>,
    previous: Option<OsString>,
    _dir: TempDir,
}

impl TempCodexHome {
    fn new() -> anyhow::Result<Self> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let guard = LOCK
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = std::env::var_os("CODEX_HOME");
        let dir = TempDir::new()?;
        unsafe {
            std::env::set_var("CODEX_HOME", dir.path());
        }
        Ok(Self {
            _guard: guard,
            previous,
            _dir: dir,
        })
    }
}

impl Drop for TempCodexHome {
    fn drop(&mut self) {
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var("CODEX_HOME", value),
                None => std::env::remove_var("CODEX_HOME"),
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[serial]
async fn oauth_login_sends_resource_in_authorization_and_token_exchange() -> anyhow::Result<()> {
    let _home = TempCodexHome::new()?;
    let server = MockResourceBoundOAuthServer::start().await?;

    let handle = perform_oauth_login_return_url(
        "resource-bound-oauth",
        &server.mcp_url,
        OAuthCredentialsStoreMode::File,
        /*http_headers*/ None,
        /*env_http_headers*/ None,
        &["openid".to_string(), "profile".to_string()],
        /*oauth_client_id*/ None,
        /*oauth_resource*/ None,
        /*timeout_secs*/ Some(5),
        /*callback_port*/ None,
        /*callback_url*/ None,
    )
    .await?;

    let auth_url = Url::parse(handle.authorization_url())?;
    assert_eq!(
        query_values(&auth_url, "resource"),
        vec![server.mcp_url.clone()]
    );

    let redirect_uri = query_values(&auth_url, "redirect_uri")
        .into_iter()
        .next()
        .expect("authorization URL includes redirect_uri");
    let state = query_values(&auth_url, "state")
        .into_iter()
        .next()
        .expect("authorization URL includes state");
    complete_callback(&redirect_uri, &state).await?;
    handle.wait().await?;

    assert_eq!(
        server.token_request_resources().await,
        vec![server.mcp_url.clone()]
    );

    Ok(())
}

fn query_values(url: &Url, key: &str) -> Vec<String> {
    url.query_pairs()
        .filter(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
        .collect()
}

async fn complete_callback(redirect_uri: &str, state: &str) -> anyhow::Result<()> {
    let mut callback_url = Url::parse(redirect_uri)?;
    callback_url
        .query_pairs_mut()
        .append_pair("code", "test-code")
        .append_pair("state", state);
    let response = reqwest::get(callback_url).await?;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    Ok(())
}

struct MockResourceBoundOAuthServer {
    handle: JoinHandle<()>,
    mcp_url: String,
    token_request_resources: Arc<AsyncMutex<Vec<String>>>,
}

#[derive(Clone)]
struct MockResourceBoundOAuthState {
    base_url: String,
    mcp_url: String,
    token_request_resources: Arc<AsyncMutex<Vec<String>>>,
}

impl MockResourceBoundOAuthServer {
    async fn start() -> anyhow::Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let base_url = format!("http://{addr}");
        let mcp_url = format!("{base_url}/api/mcp");
        let token_request_resources = Arc::new(AsyncMutex::new(Vec::new()));
        let state = MockResourceBoundOAuthState {
            base_url,
            mcp_url: mcp_url.clone(),
            token_request_resources: token_request_resources.clone(),
        };
        let app = Router::new()
            .route("/api/mcp", get(mcp_requires_auth))
            .route(
                "/.well-known/oauth-protected-resource/api/mcp",
                get(protected_resource_metadata),
            )
            .route(
                "/.well-known/oauth-authorization-server/api/auth",
                get(authorization_server_metadata),
            )
            .route("/api/auth/oauth2/register", post(register_client))
            .route("/api/auth/oauth2/token", post(exchange_token))
            .with_state(state);
        let handle = tokio::spawn(async move {
            if let Err(err) = axum::serve(listener, app).await {
                eprintln!("mock resource-bound OAuth server failed: {err}");
            }
        });

        Ok(Self {
            handle,
            mcp_url,
            token_request_resources,
        })
    }

    async fn token_request_resources(&self) -> Vec<String> {
        self.token_request_resources.lock().await.clone()
    }
}

impl Drop for MockResourceBoundOAuthServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn mcp_requires_auth(State(state): State<MockResourceBoundOAuthState>) -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        [(
            "WWW-Authenticate",
            format!(
                r#"Bearer resource_metadata="{}/.well-known/oauth-protected-resource/api/mcp""#,
                state.base_url
            ),
        )],
        "missing authorization header",
    )
}

async fn protected_resource_metadata(
    State(state): State<MockResourceBoundOAuthState>,
) -> impl IntoResponse {
    Json(json!({
        "resource": state.mcp_url,
        "authorization_servers": [format!("{}/api/auth", state.base_url)]
    }))
}

async fn authorization_server_metadata(
    State(state): State<MockResourceBoundOAuthState>,
) -> impl IntoResponse {
    Json(json!({
        "issuer": format!("{}/api/auth", state.base_url),
        "authorization_endpoint": format!("{}/api/auth/oauth2/authorize", state.base_url),
        "token_endpoint": format!("{}/api/auth/oauth2/token", state.base_url),
        "registration_endpoint": format!("{}/api/auth/oauth2/register", state.base_url),
        "response_types_supported": ["code"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
        "scopes_supported": ["openid", "profile", "email", "offline_access"]
    }))
}

async fn register_client() -> impl IntoResponse {
    (
        StatusCode::CREATED,
        Json(json!({
            "client_id": "test-client-id",
            "client_secret": null,
            "redirect_uris": ["http://127.0.0.1/callback"],
            "token_endpoint_auth_method": "none"
        })),
    )
}

async fn exchange_token(
    State(state): State<MockResourceBoundOAuthState>,
    body: String,
) -> impl IntoResponse {
    let params: HashMap<String, String> = url::form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect();
    let resource = params.get("resource").cloned();
    if resource.as_deref() != Some(state.mcp_url.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "invalid_target",
                "error_description": "missing or invalid resource"
            })),
        );
    }
    state
        .token_request_resources
        .lock()
        .await
        .push(resource.unwrap_or_default());

    (
        StatusCode::OK,
        Json(json!({
            "access_token": "test-access-token",
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "test-refresh-token",
            "scope": "openid profile"
        })),
    )
}
