//! OAuth 2.0 "get token": run the grant now and cache the token where sends
//! look (`yuki_oauth`'s cache), so the next send just uses it. Browser grants
//! (authorization code, implicit) open the auth URL and wait on a loopback.

use std::{sync::Arc, time::Duration};

use gpui_kit::*;
use tokio::sync::oneshot;
use yuki_auth::oauth2::{gen_state, gen_verifier, pkce_challenge};
use yuki_core::{HttpRequest, OAuth2Grant, OAuth2PkceMethod, YukiError};
use yuki_mcp::{api::ApiBackend, resolve};
use yuki_oauth::{flow, loopback::Loopback, token_cache, OAuth2Config};

use crate::{backend::Backend, editor::RequestEditor};

/// How long the browser login may take.
const LOGIN_WAIT: Duration = Duration::from_secs(300);

impl Backend {
    /// Runs the OAuth 2 grant of `req`'s (draft) auth. Browser grants send the
    /// URL to open on `url_tx` before waiting for the redirect.
    pub async fn oauth2_token(
        &self,
        req: HttpRequest,
        env_id: Option<String>,
        url_tx: oneshot::Sender<String>,
    ) -> Result<String, String> {
        let api = self.api.clone();
        self.rt
            .spawn(async move { fetch(api, req, env_id, url_tx).await })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }
}

async fn fetch(
    api: Arc<ApiBackend>,
    req: HttpRequest,
    env_id: Option<String>,
    url_tx: oneshot::Sender<String>,
) -> Result<String, YukiError> {
    let ws = req.workspace_id.clone();
    let api2 = api.clone();
    let req = tokio::task::spawn_blocking(move || resolve_vars(&api2, req, env_id))
        .await
        .map_err(|e| YukiError::Storage(e.to_string()))?;
    let config = OAuth2Config::from_auth(&req.auth)
        .ok_or_else(|| YukiError::InvalidConfig(t!("auth is not OAuth 2.0").into()))?;
    let encrypted = api.workspaces.get(&ws).is_ok_and(|w| w.encrypted);
    let dir = &api.app_data_dir;
    let key = config.cache_key();

    let result = match config.grant {
        OAuth2Grant::ClientCredentials | OAuth2Grant::Password => {
            // Drop the cached token so this really asks the server again.
            token_cache::clear(dir, &ws, &key)?;
            yuki_oauth::ensure_token(dir, &ws, encrypted, &config).await?;
            let cached = yuki_oauth::load_cached(dir, &ws, &key).await?;
            return Ok(summary(cached.map_or(0, |t| t.expires_at)));
        }
        OAuth2Grant::AuthorizationCode | OAuth2Grant::Implicit => {
            browser_login(&config, url_tx).await?
        }
    };
    let token = yuki_oauth::save_token(dir, &ws, encrypted, key, result).await?;
    Ok(summary(token.expires_at))
}

async fn browser_login(
    config: &OAuth2Config,
    url_tx: oneshot::Sender<String>,
) -> Result<flow::TokenResult, YukiError> {
    let lb = Loopback::bind_port(loopback_port(&config.redirect_uri)?).await?;
    let redirect = match config.redirect_uri.as_str() {
        "" => lb.redirect_uri(),
        uri => uri.to_string(),
    };
    let state = match config.state.as_str() {
        "" => gen_state(),
        s => s.to_string(),
    };
    let verifier = (config.grant == OAuth2Grant::AuthorizationCode && config.use_pkce).then(|| {
        match config.code_verifier.as_str() {
            "" => gen_verifier(),
            v => v.to_string(),
        }
    });
    let challenge = verifier
        .as_ref()
        .map(|v| match config.code_challenge_method {
            OAuth2PkceMethod::S256 => pkce_challenge(v),
            OAuth2PkceMethod::Plain => v.clone(),
        });
    let url = flow::build_auth_url(config, &redirect, &state, challenge.as_deref());
    let _ = url_tx.send(url);
    if config.grant == OAuth2Grant::Implicit {
        return lb.wait_for_token(&state, LOGIN_WAIT).await;
    }
    let code = lb.wait_for_code(&state, LOGIN_WAIT).await?;
    let client = reqwest::Client::new();
    let verifier = verifier.unwrap_or_default();
    flow::exchange_code(&client, config, code, &verifier, &redirect).await
}

/// `{{ VAR }}` resolved like a send does, so the token is cached under the
/// key the send will compute.
fn resolve_vars(api: &ApiBackend, mut req: HttpRequest, env_id: Option<String>) -> HttpRequest {
    let ws = req.workspace_id.clone();
    let dir = &api.app_data_dir;
    let envs = api.environments.list(&ws).unwrap_or_default();
    let key = yuki_crypto::load_key_from_file(&ws, dir).ok();
    let mut vars = resolve::load_env_vars_from(&envs, &ws, env_id.as_deref(), dir);
    let folders = api.requests.list_folders(&ws).unwrap_or_default();
    resolve::apply_folder_vars(&mut vars, req.folder_id.as_deref(), &folders, key.as_ref());
    resolve::apply_to_request(&mut req, &vars);
    req
}

/// Port of a loopback redirect URI (`0` = let the OS pick when there's none).
/// The token comes back to this app, so only 127.0.0.1/localhost works.
fn loopback_port(redirect_uri: &str) -> Result<u16, YukiError> {
    if redirect_uri.is_empty() {
        return Ok(0);
    }
    let rest = redirect_uri.strip_prefix("http://").unwrap_or(redirect_uri);
    let authority = rest.split('/').next().unwrap_or("");
    let (host, port) = authority.split_once(':').unwrap_or((authority, "80"));
    match (host, port.parse()) {
        ("127.0.0.1" | "localhost", Ok(port)) => Ok(port),
        _ => Err(YukiError::InvalidConfig(
            t!("redirect uri must be http://127.0.0.1:<port>/… (or empty)").into(),
        )),
    }
}

fn summary(expires_at: i64) -> String {
    match chrono::DateTime::from_timestamp(expires_at, 0).filter(|_| expires_at > 0) {
        Some(t) => t!(
            "token obtained · expires at %{time}",
            time = t.with_timezone(&chrono::Local).format("%H:%M")
        )
        .into_owned(),
        None => t!("token obtained").into_owned(),
    }
}

impl RequestEditor {
    pub(crate) fn fetch_token(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let (url_tx, url_rx) = oneshot::channel::<String>();
        let (backend, req, env) = (self.backend.clone(), self.draft(cx), self.env_id.clone());
        self.auth.status = t!("fetching token…").into_owned().into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            if let Ok(url) = url_rx.await {
                _ = this.update(cx, |e, cx| {
                    cx.open_url(&url);
                    e.auth.status = t!("waiting for browser login…").into_owned().into();
                    cx.notify();
                });
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let result = backend.oauth2_token(req, env, url_tx).await;
            _ = this.update(cx, |e, cx| {
                e.auth.status = match result {
                    Ok(s) => s.into(),
                    Err(err) => t!("failed: %{err}", err = err).into_owned().into(),
                };
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::loopback_port;

    #[test]
    fn redirect_uri_must_be_loopback() {
        assert_eq!(loopback_port("").unwrap(), 0);
        assert_eq!(
            loopback_port("http://127.0.0.1:8123/callback").unwrap(),
            8123
        );
        assert_eq!(loopback_port("http://localhost:9000").unwrap(), 9000);
        assert!(loopback_port("https://app.example.com/cb").is_err());
    }
}
