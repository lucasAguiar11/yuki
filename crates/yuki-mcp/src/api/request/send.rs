use super::super::{redact, ApiBackend};
use crate::{protocol::ToolResult, resolve};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use yuki_core::{
    AuthConfig, BodyField, BodyKind, Environment, EnvironmentKind, HttpRequest, HttpResponse,
    RequestBody, RequestParameter, StoredCookie, YukiError,
};
use yuki_storage::{CookieJarStore, RequestStore, ResponseStore, SelectionStore, WorkspaceStore};

/// Resolve + decrypt the active cookie jar for a workspace, off-runtime.
/// Mirrors Tauri's `load_active_jar_for_send` so MCP sends (and pair replays)
/// use the same jar as the UI. Soft-fails to an empty jar on I/O errors rather
/// than aborting the send.
pub(in crate::api) fn load_active_jar(
    workspaces: &WorkspaceStore,
    cookies_store: &CookieJarStore,
    selections: &SelectionStore,
    app_data_dir: &Path,
    workspace_id: &str,
) -> Result<(String, Vec<StoredCookie>), String> {
    let ws = workspaces.get(workspace_id).map_err(|e| e.to_string())?;
    // Selection if set, else the first existing jar (mirrors the app's
    // resolve_active_jar), else the auto-created default.
    let jar_id = selections
        .active_jar(workspace_id)
        .or_else(|| {
            cookies_store
                .list(workspace_id)
                .ok()
                .and_then(|jars| jars.into_iter().next().map(|j| j.id))
        })
        .unwrap_or_else(|| yuki_storage::DEFAULT_JAR_ID.to_string());
    match cookies_store.get(workspace_id, &jar_id) {
        Ok(mut jar) => {
            if yuki_cookies::crypto::jar_needs_key(&jar.cookies) {
                if !ws.encrypted {
                    return Err(
                        "workspace_encryption_required for encrypted cookie values".to_string()
                    );
                }
                let key =
                    yuki_crypto::load_key(workspace_id, app_data_dir).map_err(|e| e.to_string())?;
                yuki_cookies::crypto::decrypt_values(&mut jar.cookies, &key)
                    .map_err(|e| e.to_string())?;
            }
            Ok((jar_id, jar.cookies))
        }
        Err(e) => {
            // Soft-fail: send without cookies rather than abort. `get`
            // auto-creates the default jar, so this is really just I/O
            // failures we don't want to fatal-error on.
            eprintln!("[mcp] cookie jar load failed: {e}; sending without cookies");
            Ok((jar_id, Vec::new()))
        }
    }
}

/// Parse a `headers`/`queryParams` arg into `RequestParameter`s. Accepts an
/// object map `{ "Name": "value" }` (the common case) or an array of
/// `{ name, value, enabled? }` (for duplicate names or disabled rows).
pub(super) fn parse_params(v: &Value) -> Vec<RequestParameter> {
    let mk = |name: String, value: String, enabled: bool| RequestParameter {
        id: format!("p_{}", yuki_core::new_id()),
        name,
        value,
        enabled,
    };
    match v {
        Value::Object(map) => map
            .iter()
            .filter_map(|(k, val)| val.as_str().map(|s| mk(k.clone(), s.to_string(), true)))
            .collect(),
        Value::Array(arr) => arr
            .iter()
            .filter_map(|item| {
                let name = item["name"].as_str()?;
                Some(mk(
                    name.to_string(),
                    item["value"].as_str().unwrap_or("").to_string(),
                    item["enabled"].as_bool().unwrap_or(true),
                ))
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Parse a `body` arg into a `RequestBody`. `Ok(None)` = no body arg given.
/// Supports raw kinds (json/xml/text/html via `text`) and form-urlencoded (via
/// a `fields` object map). Multipart / binary file uploads are intentionally
/// unsupported over MCP (they'd read arbitrary local files); GraphQL bodies go
/// through `graphqlQuery`.
pub(super) fn parse_body(v: &Value) -> Result<Option<RequestBody>, String> {
    if v.is_null() {
        return Ok(None);
    }
    let kind_str = v["kind"]
        .as_str()
        .ok_or("body.kind is required (json, xml, text, html, form_url_encoded, none)")?;
    let kind = match kind_str.to_ascii_lowercase().as_str() {
        "json" => BodyKind::Json,
        "xml" => BodyKind::Xml,
        "text" => BodyKind::Text,
        "html" => BodyKind::Html,
        "form_url_encoded" | "form" => BodyKind::FormUrlEncoded,
        "none" => BodyKind::None,
        other => {
            return Err(format!(
                "unsupported body kind {other:?} — use json, xml, text, html, form_url_encoded, \
                 or none (for GraphQL use graphqlQuery; multipart/binary uploads aren't supported)"
            ))
        }
    };
    let fields = if matches!(kind, BodyKind::FormUrlEncoded) {
        v["fields"].as_object().map(|m| {
            m.iter()
                .filter_map(|(k, val)| {
                    val.as_str().map(|s| BodyField {
                        id: format!("f_{}", yuki_core::new_id()),
                        name: k.clone(),
                        value: s.to_string(),
                        enabled: true,
                        is_file: false,
                        content_type: None,
                    })
                })
                .collect()
        })
    } else {
        None
    };
    Ok(Some(RequestBody {
        kind,
        text: v["text"].as_str().unwrap_or("").to_string(),
        fields,
        content_type: v["contentType"].as_str().map(str::to_string),
        ..Default::default()
    }))
}

/// Decrypt → upsert captured cookies by RFC 6265 identity → re-encrypt → save.
/// Mirrors the Tauri `ingest_captured_cookies` so MCP and IPC captures match.
fn ingest_captured_blocking(
    workspaces: &WorkspaceStore,
    cookies_store: &CookieJarStore,
    app_data_dir: &Path,
    workspace_id: &str,
    jar_id: &str,
    captured: &[StoredCookie],
) -> Result<(), YukiError> {
    if captured.is_empty() {
        return Ok(());
    }
    let ws = workspaces.get(workspace_id)?;
    let mut jar = cookies_store.get(workspace_id, jar_id)?;

    // Need the key to decrypt existing cookies, or to encrypt newly captured
    // ones on save when ws.encrypted (even if the jar is currently plaintext).
    let needs_key = yuki_cookies::crypto::jar_needs_key(&jar.cookies) || ws.encrypted;
    let key = if needs_key {
        if !ws.encrypted {
            return Err(YukiError::InvalidConfig(
                "workspace_encryption_required".to_string(),
            ));
        }
        Some(yuki_crypto::load_key(workspace_id, app_data_dir)?)
    } else {
        None
    };
    if let Some(ref k) = key {
        yuki_cookies::crypto::decrypt_values(&mut jar.cookies, k)?;
    }

    for fresh in captured {
        let pos = jar
            .cookies
            .iter()
            .position(|c| yuki_cookies::matching::same_identity(c, fresh));
        let mut entry = fresh.clone();
        entry.value_encrypted = ws.encrypted;
        if let Some(idx) = pos {
            entry.id = jar.cookies[idx].id.clone();
            entry.created_at = jar.cookies[idx].created_at.clone();
            jar.cookies[idx] = entry;
        } else {
            jar.cookies.push(entry);
        }
    }
    jar.updated_at = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.6f")
        .to_string();
    if let Some(ref k) = key {
        yuki_cookies::crypto::encrypt_values(&mut jar.cookies, k)?;
    }
    cookies_store.save(&jar)?;
    Ok(())
}

/// What the caller picks over the stored request: a URL (MCP `urlOverride`)
/// and a variation (id or name; `None` = the active one).
#[derive(Default)]
struct Pick {
    url_override: Option<String>,
    variation: Option<String>,
}

impl ApiBackend {
    pub(in crate::api) async fn request_send(&self, args: &Value) -> ToolResult {
        let ws_id = require!(args, "workspaceId");
        let req_id = require!(args, "requestId");
        let env_id = args["environmentId"].as_str().map(str::to_string);
        let url_override = args["urlOverride"].as_str().map(str::to_string);
        let variation = args["variation"].as_str().map(str::to_string);
        eprintln!(
            "[mcp] request.send ws={ws_id} req={req_id} env={env_id:?} variation={variation:?}"
        );

        // 30s cap so the MCP client never hangs indefinitely.
        let send = self.send_http(ws_id, req_id, env_id, url_override, variation, true);
        match tokio::time::timeout(std::time::Duration::from_secs(30), send).await {
            Err(_) => ToolResult::error(
                "Request timed out after 30 seconds. \
                 Check the URL and that the server is reachable.",
            ),
            Ok(Err(msg)) => ToolResult::error(msg),
            Ok(Ok(resp)) => ToolResult::json(&resp),
        }
    }

    /// Full send pipeline shared by MCP and the app: resolve env/folder vars and
    /// auth, attach the active cookie jar, send, ingest captured cookies, append
    /// to history. `agent` = AI-initiated: requires an explicit env when personal
    /// ones exist and refuses link-local / cloud-metadata targets.
    ///
    /// `$response.body` is a pointer: every request whose response this one
    /// reads (through its fields or the variables it uses) is sent first, so the
    /// value is the current one. A source already being sent up the chain (a
    /// cycle) is read from history instead.
    ///
    /// `variation` (id or name) picks which variation of the request is sent;
    /// `None` = its active one. Sources always send their own active one.
    pub async fn send_http(
        &self,
        ws_id: String,
        req_id: String,
        env_id: Option<String>,
        url_override: Option<String>,
        variation: Option<String>,
        agent: bool,
    ) -> Result<HttpResponse, String> {
        let pick = Pick {
            url_override,
            variation,
        };
        self.send_chain(ws_id, req_id, env_id, pick, agent, Vec::new())
            .await
    }

    fn send_chain(
        &self,
        ws_id: String,
        req_id: String,
        env_id: Option<String>,
        pick: Pick,
        agent: bool,
        chain: Vec<String>,
    ) -> Pin<Box<dyn Future<Output = Result<HttpResponse, String>> + Send + '_>> {
        Box::pin(async move {
            let mut chain = chain;
            chain.push(req_id.clone());
            for src in self
                .response_sources(
                    &ws_id,
                    &req_id,
                    env_id.as_deref(),
                    pick.variation.as_deref(),
                )
                .await
            {
                if chain.contains(&src) {
                    continue;
                }
                let name = self
                    .requests
                    .get_request(&ws_id, &src)
                    .map_or_else(|_| src.clone(), |r| r.name);
                self.send_chain(
                    ws_id.clone(),
                    src,
                    env_id.clone(),
                    Pick::default(),
                    agent,
                    chain.clone(),
                )
                .await
                .map_err(|e| format!("{name} (read by $response.body): {e}"))?;
            }
            self.send_once(ws_id, req_id, env_id, pick, agent).await
        })
    }

    /// `$response.body` sources of a stored request (see `resolve::response_sources`).
    async fn response_sources(
        &self,
        ws_id: &str,
        req_id: &str,
        env_id: Option<&str>,
        variation: Option<&str>,
    ) -> Vec<String> {
        let (requests, environments) = (self.requests.clone(), self.environments.clone());
        let app_data_dir = self.app_data_dir.clone();
        let (ws, id, env, variation) = (
            ws_id.to_string(),
            req_id.to_string(),
            env_id.map(str::to_string),
            variation.map(str::to_string),
        );
        let workspaces = self.workspaces.clone();
        tokio::task::spawn_blocking(move || {
            let Ok(mut req) = requests
                .get_request(&ws, &id)
                .and_then(|r| r.effective(variation.as_deref()))
            else {
                return Vec::new();
            };
            let envs = environments.list(&ws).unwrap_or_default();
            let key = yuki_crypto::load_key_from_file(&ws, &app_data_dir).ok();
            let mut vars = resolve::load_env_vars_from(&envs, &ws, env.as_deref(), &app_data_dir);
            let folders = requests.list_folders(&ws).unwrap_or_default();
            resolve::apply_folder_vars(&mut vars, req.folder_id.as_deref(), &folders, key.as_ref());
            // Folder/workspace auth and headers can read a response too.
            inherit(&mut req, &folders, &workspaces, key.as_ref());
            resolve::response_sources(&vars, &req)
        })
        .await
        .unwrap_or_default()
    }

    async fn send_once(
        &self,
        ws_id: String,
        req_id: String,
        env_id: Option<String>,
        pick: Pick,
        agent: bool,
    ) -> Result<HttpResponse, String> {
        let Pick {
            url_override,
            variation,
        } = pick;
        let environments = self.environments.clone();
        let requests = self.requests.clone();
        let workspaces = self.workspaces.clone();
        let cookies_store = self.cookies.clone();
        let selections = self.selections.clone();
        let responses = self.responses.clone();
        let app_data_dir = self.app_data_dir.clone();
        let ws_id2 = ws_id.clone();
        let req_id2 = req_id.clone();
        let env_id2 = env_id.clone();
        let t0 = std::time::Instant::now();

        let prepared = tokio::task::spawn_blocking(move || -> Result<_, String> {
            let mut req = requests
                .get_request(&ws_id2, &req_id2)
                .and_then(|r| r.effective(variation.as_deref()))
                .map_err(|e| e.to_string())?;
            // As edited (templates kept, variation applied): what "restore" brings back.
            let edited = req.clone();
            let envs = environments.list(&ws_id2).unwrap_or_default();

            // Require an explicit env when personal ones exist: defaulting to
            // globals would leave their {{ VAR }} tokens unresolved.
            if agent && env_id2.is_none() {
                let personal: Vec<_> = envs
                    .iter()
                    .filter(|e| e.kind == EnvironmentKind::Personal)
                    .collect();
                if !personal.is_empty() {
                    let list = personal
                        .iter()
                        .map(|e| format!("  • {} (id: {})", e.name, e.id))
                        .collect::<Vec<_>>()
                        .join("\n");
                    return Err(format!(
                        "environmentId is required when personal environments exist.\n\
                         Please re-call with one of the following:\n{list}"
                    ));
                }
            }

            let (vars, var_key, stored_auth) = resolve_request(
                &requests,
                &workspaces,
                &envs,
                &responses,
                &app_data_dir,
                env_id2.as_deref(),
                &mut req,
            );

            if let Some(url) = url_override {
                req.url = url;
            }

            let (jar_id, mut cookies) = load_active_jar(
                &workspaces,
                &cookies_store,
                &selections,
                &app_data_dir,
                &ws_id2,
            )?;

            // Resolve `{{ VAR }}`, strip encrypt() chips, decrypt `enc:v1:` in
            // cookie fields — same steps as Tauri `send_request` for one wire form.
            yuki_cookies::resolve::resolve_cookies(&mut cookies, &vars, var_key.as_ref());

            Ok((req, cookies, jar_id, stored_auth, edited))
        })
        .await;

        let (mut req, attach_cookies, jar_id, stored_auth, edited) = match prepared {
            Ok(r) => r?,
            Err(e) => return Err(format!("Internal error: {e}")),
        };

        self.apply_oauth2(&mut req).await?;

        // `req` is now fully resolved (literal URL/headers/body, no `{{ }}`) —
        // the authoritative "as sent" record. Cloned here, before the executor
        // runs, so a later "save as pair" can promote it without re-resolving
        // (which would regenerate non-deterministic template fns like
        // `uuid.v4()`).
        let mut resolved_request = req.clone();
        // Static / OAuth2 auth was folded into a header above, leaving `req.auth`
        // as None. Restore the original config on the snapshot so its AUTH tab
        // shows the real scheme — replay ignores it (the executor only applies
        // `is_dynamic()` auth; the folded header carries the static secret).
        if matches!(req.auth, AuthConfig::None)
            && stored_auth.is_active()
            && !stored_auth.is_dynamic()
        {
            resolved_request.auth = stored_auth;
        }

        eprintln!(
            "[send] prepared in {:.0}ms — sending {} {} (jar={jar_id}, {} cookie(s))",
            t0.elapsed().as_secs_f64() * 1000.0,
            req.method,
            redact::redact_url(&req.url),
            attach_cookies.len(),
        );

        let dns_overrides = self
            .workspaces
            .get(&ws_id)
            .map(|w| w.dns_overrides)
            .unwrap_or_default();
        let result = if agent {
            self.executor
                .send_guarded(&req, attach_cookies, dns_overrides)
                .await
        } else {
            self.executor
                .send(&req, attach_cookies, dns_overrides)
                .await
        };
        let resp = match result {
            Ok(r) => r,
            Err(YukiError::Cancelled) => return Err("Request cancelled".into()),
            Err(e) => return Err(redact::redact_error(&e.to_string())),
        };
        eprintln!(
            "[send] response {} in {:.0}ms (total wall {:.0}ms)",
            resp.status,
            resp.timing.total_ms,
            t0.elapsed().as_secs_f64() * 1000.0
        );

        // Block on cookie ingest: a follow-up send must see captured cookies
        // (login, then hit a protected endpoint).
        if !resp.captured_cookies.is_empty() {
            let captured = resp.captured_cookies.clone();
            let workspaces = self.workspaces.clone();
            let cookies_store = self.cookies.clone();
            let app_data_dir = self.app_data_dir.clone();
            let ws_for_ingest = ws_id.clone();
            let ingest = tokio::task::spawn_blocking(move || {
                ingest_captured_blocking(
                    &workspaces,
                    &cookies_store,
                    &app_data_dir,
                    &ws_for_ingest,
                    &jar_id,
                    &captured,
                )
            })
            .await;
            match ingest {
                Ok(Ok(())) => self.notify_cookies(&ws_id),
                Ok(Err(e)) => eprintln!("[send] failed to ingest captured cookies: {e}"),
                Err(e) => eprintln!("[send] cookie ingest task panicked: {e}"),
            }
        }

        // Awaited so the returned response carries its history id (callers
        // list/select history right after a send).
        let mut resp = resp;
        let responses = self.responses.clone();
        let resp_clone = resp.clone();
        let (ws, req) = (ws_id.clone(), req_id.clone());
        let stored = tokio::task::spawn_blocking(move || {
            responses.append(&ws, &req, resp_clone, resolved_request, Some(edited), 20)
        })
        .await;
        match stored {
            Ok(Ok(stored)) => {
                resp.response_id = stored.id;
                (self.notify)(
                    "mcp:response:stored",
                    serde_json::json!({ "workspaceId": ws_id, "requestId": req_id }),
                );
            }
            Ok(Err(e)) => eprintln!("[send] failed to store response history: {e}"),
            Err(e) => eprintln!("[send] history task panicked: {e}"),
        }
        Ok(resp)
    }
}

/// The blocking half of resolution: env + folder vars, inherited headers and
/// auth, decrypted auth secrets, `{{ }}` applied (static auth folded into a
/// header). Returns the vars and the workspace key (cookies reuse both) and the
/// pre-fold auth.
fn resolve_request(
    requests: &RequestStore,
    workspaces: &WorkspaceStore,
    envs: &[Environment],
    responses: &ResponseStore,
    app_data_dir: &Path,
    env_id: Option<&str>,
    req: &mut HttpRequest,
) -> (HashMap<String, String>, Option<[u8; 32]>, AuthConfig) {
    let ws_id = req.workspace_id.clone();
    // File-only key (the keychain can block 60s headless). Shared by
    // folder-var and cookie decryption.
    let var_key = yuki_crypto::load_key_from_file(&ws_id, app_data_dir).ok();

    // Folder vars layer over env vars (nearest folder wins).
    let mut vars = resolve::load_env_vars_from(envs, &ws_id, env_id, app_data_dir);
    let folders = requests.list_folders(&ws_id).unwrap_or_default();
    resolve::apply_folder_vars(
        &mut vars,
        req.folder_id.as_deref(),
        &folders,
        var_key.as_ref(),
    );
    inherit(req, &folders, workspaces, var_key.as_ref());
    // After `inherit`: folder/workspace auth and headers can read a response.
    resolve::prefill_responses(&mut vars, Some(req), &ws_id, responses);
    // Original (resolved, secrets-decrypted) auth, captured before
    // `apply_to_request` folds static schemes into a header and clears
    // `req.auth` — kept for the snapshot's AUTH tab.
    let stored_auth = req.auth.clone();
    resolve::apply_to_request(req, &vars);
    (vars, var_key, stored_auth)
}

/// What a request inherits, in place: headers merged by name (own, then the
/// nearest folder up to the workspace), `inherit` auth resolved, auth secrets
/// decrypted (folder/workspace secrets are sealed with the same key; static
/// schemes must inject plaintext, SigV4 sign with it).
fn inherit(
    req: &mut HttpRequest,
    folders: &[yuki_core::ApiFolder],
    workspaces: &WorkspaceStore,
    key: Option<&[u8; 32]>,
) {
    let ws = workspaces.get(&req.workspace_id).ok();
    let (ws_headers, ws_auth) = ws.map(|w| (w.headers, w.auth)).unwrap_or_default();
    let folder = req.folder_id.as_deref();
    req.headers = resolve::merge_inherited_metadata(&req.headers, folder, folders, &ws_headers);
    req.auth = resolve::effective_auth(&req.auth, folder, folders, &ws_auth);
    if let Some(k) = key {
        for (secret, encrypted) in req.auth.secret_fields_mut() {
            if encrypted && yuki_crypto::is_encrypted(secret) {
                if let Ok(plain) = yuki_crypto::decrypt(secret, k) {
                    *secret = plain;
                }
            }
        }
    }
}

impl ApiBackend {
    /// OAuth 2.0 resolves to a Bearer from the shared token cache: the
    /// non-interactive grants fetch/refresh as needed; authorization_code
    /// reuses a token previously acquired via the UI (it can't open a browser
    /// here). `apply_to_request` already expanded its `{{ }}` fields. Always
    /// `Bearer` — non-Bearer types (MAC/DPoP) need real protocol support, not
    /// a scheme swap, so the cached `token_type` is inspector-only.
    async fn apply_oauth2(&self, req: &mut HttpRequest) -> Result<(), String> {
        if !matches!(req.auth, AuthConfig::OAuth2 { .. }) {
            return Ok(());
        }
        if req.auth.is_active() {
            if let Some(config) = yuki_oauth::OAuth2Config::from_auth(&req.auth) {
                let ws_id = &req.workspace_id;
                let encrypted = self
                    .workspaces
                    .get(ws_id)
                    .map(|w| w.encrypted)
                    .unwrap_or(false);
                let token = yuki_oauth::ensure_token(&self.app_data_dir, ws_id, encrypted, &config)
                    .await
                    .map_err(|e| format!("OAuth 2.0: {e}"))?;
                req.headers.push(resolve::auth_header(
                    "Authorization",
                    format!("Bearer {token}"),
                ));
            }
        }
        req.auth = AuthConfig::None;
        Ok(())
    }

    /// `req` (a draft, need not be saved) as `send_http` would put it on the
    /// wire, without sending: literal URL/headers/body, its active variation
    /// applied. Dynamic auth (SigV4, Digest…) stays on `req.auth` — only the
    /// executor can apply it. The cookie jar isn't attached.
    pub async fn resolve_http(
        &self,
        req: HttpRequest,
        env_id: Option<String>,
    ) -> Result<HttpRequest, String> {
        let mut req = req.shown();
        let (requests, environments) = (self.requests.clone(), self.environments.clone());
        let workspaces = self.workspaces.clone();
        let responses = self.responses.clone();
        let app_data_dir = self.app_data_dir.clone();
        req = tokio::task::spawn_blocking(move || {
            let envs = environments.list(&req.workspace_id).unwrap_or_default();
            resolve_request(
                &requests,
                &workspaces,
                &envs,
                &responses,
                &app_data_dir,
                env_id.as_deref(),
                &mut req,
            );
            req
        })
        .await
        .map_err(|e| format!("Internal error: {e}"))?;
        self.apply_oauth2(&mut req).await?;
        Ok(req)
    }
}
