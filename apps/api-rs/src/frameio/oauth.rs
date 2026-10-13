
//! Adobe IMS OAuth for Frame.io — port of `portal.frameio.oauth` (authorization-code flow).
//! Endpoints/behaviour per CLAUDE.md: `AdobeID` must be requested on refresh too, or the
//! refreshed token is 401'd by the Frame.io API an hour after connect.

use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;

pub const IMS_AUTHORIZE_URL: &str = "https://ims-na1.adobelogin.com/ims/authorize/v2";
pub const IMS_TOKEN_URL: &str = "https://ims-na1.adobelogin.com/ims/token/v3";
pub const FRAMEIO_API_BASE: &str = "https://api.frame.io/v4";

#[derive(Debug, Clone)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub scopes: String,
}

#[derive(Debug, Clone)]
pub struct Identity {
    pub frameio_user_id: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug)]
pub struct FrameioOAuthError(pub String);
impl std::fmt::Display for FrameioOAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for FrameioOAuthError {}

/// OAuth config: client id/secret from `app_settings`, scopes/redirect from env (as in Python).
#[derive(Clone, Debug)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub scopes: String,
    pub redirect_uri: String,
}

fn to_token_set(token: &Value, default_scopes: &str) -> TokenSet {
    let expires_in = token
        .get("expires_in")
        .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
        .unwrap_or(0);
    TokenSet {
        access_token: token.get("access_token").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        refresh_token: token.get("refresh_token").and_then(|v| v.as_str()).map(|s| s.to_string()),
        expires_at: Utc::now() + Duration::seconds(expires_in),
        scopes: token
            .get("scope")
            .and_then(|v| v.as_str())
            .unwrap_or(default_scopes)
            .to_string(),
    }
}

/// Authorization URL the admin's browser is redirected to.
pub fn build_authorize_url(config: &OAuthConfig, state: &str) -> String {
    let mut url = url::Url::parse(IMS_AUTHORIZE_URL).expect("valid url");
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &config.client_id)
        .append_pair("redirect_uri", &config.redirect_uri)
        .append_pair("scope", &config.scopes)
        .append_pair("state", state);
    url.to_string()
}

fn basic_auth(config: &OAuthConfig) -> String {
    let raw = format!("{}:{}", config.client_id, config.client_secret);
    format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(raw))
}

pub async fn exchange_code(
    http: &reqwest::Client,
    config: &OAuthConfig,
    code: &str,
) -> Result<TokenSet, FrameioOAuthError> {
    let form = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}",
        urlencoding(code),
        urlencoding(&config.redirect_uri)
    );
    let resp = http
        .post(IMS_TOKEN_URL)
        .header(reqwest::header::AUTHORIZATION, basic_auth(config))
        .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
        .map_err(|e| FrameioOAuthError(format!("Token exchange failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(FrameioOAuthError(format!("Token exchange failed: {}", resp.status())));
    }
    let token: Value = resp.json().await.map_err(|_| FrameioOAuthError("Token exchange failed".into()))?;
    Ok(to_token_set(&token, &config.scopes))
}

pub async fn refresh_tokens(
    http: &reqwest::Client,
    config: &OAuthConfig,
    refresh_token: &str,
) -> Result<TokenSet, FrameioOAuthError> {
    // Scope is sent explicitly: without `AdobeID` here, the refreshed token is rejected by
    // Frame.io ~1h after connect (Adobe adds it on consent, not on refresh).
    let form = format!(
        "grant_type=refresh_token&refresh_token={}&scope={}",
        urlencoding(refresh_token),
        urlencoding(&config.scopes)
    );
    let resp = http
        .post(IMS_TOKEN_URL)
        .header(reqwest::header::AUTHORIZATION, basic_auth(config))
        .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
        .map_err(|e| FrameioOAuthError(format!("Token refresh failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(FrameioOAuthError(format!("Token refresh failed: {}", resp.status())));
    }
    let token: Value = resp.json().await.map_err(|_| FrameioOAuthError("Token refresh failed".into()))?;
    Ok(to_token_set(&token, &config.scopes))
}

/// GET /v4/me — verifies the token and returns the connected identity.
pub async fn fetch_identity(
    http: &reqwest::Client,
    access_token: &str,
) -> Result<Identity, FrameioOAuthError> {
    let resp = http
        .get(format!("{FRAMEIO_API_BASE}/me"))
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {access_token}"))
        .send()
        .await
        .map_err(|e| FrameioOAuthError(format!("Could not fetch Frame.io identity: {e}")))?;
    if !resp.status().is_success() {
        return Err(FrameioOAuthError("Could not fetch Frame.io identity".into()));
    }
    let body: Value = resp.json().await.map_err(|_| FrameioOAuthError("Bad identity body".into()))?;
    let data = body.get("data").unwrap_or(&body);
    Ok(Identity {
        frameio_user_id: data.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()),
        email: data.get("email").and_then(|v| v.as_str()).map(|s| s.to_string()),
    })
}

/// Percent-encode a form value (application/x-www-form-urlencoded).
fn urlencoding(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}
