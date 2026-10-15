use super::super::{redact, ApiBackend};
use super::send::{parse_body, parse_params};
use crate::protocol::ToolResult;
use serde_json::Value;
use std::path::Path;
use yuki_core::{AuthConfig, HttpRequest, YukiError};
use yuki_storage::WorkspaceStore;

/// A request as the agent sees it: the YAML fields plus its markdown notes
/// (`req_{id}.md`, `""` when none).
fn with_notes(req: &HttpRequest, notes: String) -> Value {
    let mut v = serde_json::to_value(req).unwrap_or_default();
    v["notes"] = Value::String(notes);
    v
}

/// The `auth` tool arg. Parsed up front (cheap, sync) so a bad-JSON error is
/// immediate and serde never runs on the blocking pool. `None` = absent.
pub(in crate::api) fn auth_arg(args: &Value) -> Result<Option<AuthConfig>, String> {
    if args["auth"].is_null() {
        return Ok(None);
    }
    serde_json::from_value(args["auth"].clone())
        .map(Some)
        .map_err(|e| format!("invalid auth: {e}"))
}

/// `new` as it should replace `old` on disk (request, folder or workspace):
/// secrets echoed back masked keep `old`'s value, and they're sealed when the
/// workspace is encrypted.
pub(in crate::api) fn auth_to_store(
    workspaces: &WorkspaceStore,
    app_data_dir: &Path,
    ws: &str,
    mut new: AuthConfig,
    old: &mut AuthConfig,
) -> Result<AuthConfig, YukiError> {
    redact::restore_masked(&mut new, old);
    if workspaces.get(ws).map(|w| w.encrypted).unwrap_or(false) {
        new.mark_secrets_encrypted();
        if new.secret_fields_mut().iter().any(|(_, enc)| *enc) {
            let key = yuki_crypto::load_key(ws, app_data_dir)?;
            for (secret, enc) in new.secret_fields_mut() {
                if enc && !yuki_crypto::is_encrypted(secret) {
                    *secret = yuki_crypto::encrypt(secret, &key)?;
                }
            }
        }
    }
    Ok(new)
}

impl ApiBackend {
    pub(in crate::api) async fn request_list(&self, args: &Value) -> ToolResult {
        let ws_id = require!(args, "workspaceId");
        let reveal = redact::reveal(args);
        let requests = self.requests.clone();
        super::super::run_blocking(move || {
            let reqs = requests.list_requests(&ws_id);
            let folders = requests.list_folders(&ws_id);
            match (reqs, folders) {
                (Ok(mut reqs), Ok(mut folders)) => {
                    if !reveal {
                        for r in reqs.iter_mut() {
                            redact::mask_auth(&mut r.auth);
                        }
                        for f in folders.iter_mut() {
                            redact::mask_auth(&mut f.auth);
                        }
                    }
                    ToolResult::json(&serde_json::json!({ "requests": reqs, "folders": folders }))
                }
                (Err(e), _) | (_, Err(e)) => ToolResult::error(e.to_string()),
            }
        })
        .await
    }

    pub(in crate::api) async fn request_get(&self, args: &Value) -> ToolResult {
        let ws_id = require!(args, "workspaceId");
        let req_id = require!(args, "requestId");
        let reveal = redact::reveal(args);
        let requests = self.requests.clone();
        super::super::run_blocking(move || {
            let result = requests
                .get_request(&ws_id, &req_id)
                .and_then(|req| Ok((requests.get_notes(&ws_id, &req_id)?, req)));
            match result {
                Ok((notes, mut req)) => {
                    if !reveal {
                        redact::mask_auth(&mut req.auth);
                    }
                    ToolResult::json(&with_notes(&req, notes))
                }
                Err(e) => ToolResult::error(e.to_string()),
            }
        })
        .await
    }

    pub(in crate::api) async fn request_create(&self, args: &Value) -> ToolResult {
        let ws_id = require!(args, "workspaceId");
        let name = require!(args, "name");
        let method = require!(args, "method");
        let url = require!(args, "url");
        let folder_id = args["folderId"].as_str().map(str::to_string);
        let notes = args["notes"].as_str().unwrap_or_default().to_string();
        let headers = parse_params(&args["headers"]);
        let query_params = parse_params(&args["queryParams"]);
        let body = match parse_body(&args["body"]) {
            Ok(b) => b,
            Err(e) => return ToolResult::error(e),
        };
        let requests = self.requests.clone();
        let ws = ws_id.clone();
        let result = super::super::blocking(move || -> Result<Value, YukiError> {
            let mut req = requests.create_request(ws.clone(), folder_id, name, method, url)?;
            requests.save_notes(&ws, &req.id, &notes)?;
            // create_request only sets name/method/url; persist any extras in a
            // follow-up update so an AI can author a full request in one call.
            if !headers.is_empty() || !query_params.is_empty() || body.is_some() {
                if !headers.is_empty() {
                    req.headers = headers;
                }
                if !query_params.is_empty() {
                    req.parameters = query_params;
                }
                if body.is_some() {
                    req.body = body;
                }
                requests.update_request(
                    &ws,
                    &req.id,
                    req.method.clone(),
                    req.url.clone(),
                    req.parameters.clone(),
                    req.headers.clone(),
                    req.body.clone(),
                    req.auth.clone(),
                )?;
            }
            Ok(with_notes(&req, notes))
        })
        .await;
        match result {
            Ok(req) => {
                self.notify_requests(&ws_id);
                ToolResult::json(&req)
            }
            Err(e) => ToolResult::error(e.to_string()),
        }
    }

    pub(in crate::api) async fn request_update(&self, args: &Value) -> ToolResult {
        let ws_id = require!(args, "workspaceId");
        let req_id = require!(args, "requestId");

        let new_auth = match auth_arg(args) {
            Ok(a) => a,
            Err(e) => return ToolResult::error(e),
        };

        let name = args["name"].as_str().map(str::to_string);
        let method = args["method"].as_str().map(str::to_string);
        let url = args["url"].as_str().map(str::to_string);
        // Absent = keep; `""` clears (removes the .md).
        let notes = args["notes"].as_str().map(str::to_string);
        let graphql_query = args["graphqlQuery"].as_str().map(str::to_string);
        let graphql_vars = args["graphqlVariables"].as_str().map(str::to_string);
        // None = arg absent (preserve existing); Some(..) replaces wholesale.
        let headers = (!args["headers"].is_null()).then(|| parse_params(&args["headers"]));
        let query_params =
            (!args["queryParams"].is_null()).then(|| parse_params(&args["queryParams"]));
        let body = match parse_body(&args["body"]) {
            Ok(b) => b,
            Err(e) => return ToolResult::error(e),
        };

        let requests = self.requests.clone();
        let workspaces = self.workspaces.clone();
        let app_data_dir = self.app_data_dir.clone();
        let ws = ws_id.clone();
        let result = super::super::blocking(move || -> Result<Value, YukiError> {
            let mut req = requests.get_request(&ws, &req_id)?;
            // Rename is separate: the filename stays `req_{id}.yaml`.
            if let Some(n) = name {
                requests.rename_request(&ws, &req_id, n.clone())?;
                req.name = n;
            }
            if let Some(m) = method {
                req.method = m;
            }
            if let Some(u) = url {
                req.url = u;
            }
            if let Some(q) = graphql_query {
                req.body = Some(yuki_core::RequestBody {
                    kind: yuki_core::BodyKind::Graphql,
                    text: q,
                    graphql_variables: graphql_vars,
                    ..Default::default()
                });
                if req.method.eq_ignore_ascii_case("GET") {
                    req.method = "POST".to_string();
                }
            } else if let (Some(v), Some(body)) = (graphql_vars, req.body.as_mut()) {
                if matches!(body.kind, yuki_core::BodyKind::Graphql) {
                    body.graphql_variables = Some(v);
                }
            }
            if let Some(h) = headers {
                req.headers = h;
            }
            if let Some(p) = query_params {
                req.parameters = p;
            }
            if let Some(b) = body {
                req.body = Some(b);
            }
            if let Some(auth) = new_auth {
                req.auth = auth_to_store(&workspaces, &app_data_dir, &ws, auth, &mut req.auth)?;
            }
            requests.update_request(
                &ws,
                &req_id,
                req.method.clone(),
                req.url.clone(),
                req.parameters.clone(),
                req.headers.clone(),
                req.body.clone(),
                req.auth.clone(),
            )?;
            if let Some(n) = &notes {
                requests.save_notes(&ws, &req_id, n)?;
            }
            Ok(with_notes(&req, requests.get_notes(&ws, &req_id)?))
        })
        .await;

        match result {
            Ok(req) => {
                self.notify_requests(&ws_id);
                ToolResult::json(&req)
            }
            Err(e) => ToolResult::error(e.to_string()),
        }
    }

    pub(in crate::api) async fn request_duplicate(&self, args: &Value) -> ToolResult {
        let ws_id = require!(args, "workspaceId");
        let req_id = require!(args, "requestId");
        let requests = self.requests.clone();
        let ws = ws_id.clone();
        match super::super::blocking(move || requests.duplicate_request(&ws, &req_id)).await {
            Ok(req) => {
                self.notify_requests(&ws_id);
                ToolResult::json(&req)
            }
            Err(e) => ToolResult::error(e.to_string()),
        }
    }

    pub(in crate::api) async fn request_delete(&self, args: &Value) -> ToolResult {
        let ws_id = require!(args, "workspaceId");
        let req_id = require!(args, "requestId");
        let requests = self.requests.clone();
        let ws = ws_id.clone();
        let rid = req_id.clone();
        match super::super::blocking(move || requests.delete_request(&ws, &rid)).await {
            Ok(()) => {
                self.notify_requests(&ws_id);
                ToolResult::json(&serde_json::json!({ "deleted": req_id }))
            }
            Err(e) => ToolResult::error(e.to_string()),
        }
    }
}
