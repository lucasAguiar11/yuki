//! Core types → Yaak data export (`yaakSchema: 4`), the shape `yuki-import`'s
//! Yaak parser reads back. One bundle maps onto the Yaak workspace itself;
//! several become one folder each (carrying the workspace auth/headers) under a
//! combined workspace. Templates `{{ x }}` become Yaak's `${[ x ]}`.

use std::borrow::Cow;

use serde_json::{json, Value};
use yuki_core::{
    now_iso, ApiKeyLocation, AuthConfig, BodyKind, HttpRequest, InheritSource, OAuth1Location,
    OAuth1Signature, OAuth2ClientAuth, OAuth2Grant, OAuth2PkceMethod, RequestBody,
    RequestParameter, YukiError,
};

use crate::postman::{contains_path_parameter, effective_inherited_auth};
use crate::{
    children, http_auths, implied_content_type, rewrite_templates, split_envs,
    warn_content_type_overrides, warn_count, warn_skipped_items, Bundle, ExportResult, Node,
};

struct Out<'a> {
    workspace_id: String,
    folders: Vec<Value>,
    requests: Vec<Value>,
    warnings: &'a mut Vec<String>,
}

pub fn to_yaak(bundles: &[Bundle]) -> Result<ExportResult, YukiError> {
    let mut warnings = Vec::new();
    warn_skipped_items(bundles, "Yaak", &mut warnings);
    warn_content_type_overrides(bundles, &mut warnings);
    let oauth1_query = http_auths(bundles)
        .filter(|a| a.is_active())
        .filter(|a| {
            matches!(a, AuthConfig::OAuth1 { params_location, .. } if *params_location == OAuth1Location::Query)
        })
        .count();
    warn_count(
        &mut warnings,
        oauth1_query,
        "OAuth1 config(s) put their params in the query; Yaak signs into the Authorization header.",
    );

    let single = match bundles {
        [b] => Some(b),
        _ => None,
    };
    let workspace_id = single.map_or_else(
        || "wk_yuki_export".to_string(),
        |b| format!("wk_{}", b.workspace.id),
    );
    let mut workspace = json!({
        "model": "workspace",
        "id": workspace_id,
        "name": single.map_or("Yuki Export", |b| b.workspace.name.as_str()),
        "headers": single.map_or_else(Vec::new, |b| headers(&b.workspace.headers)),
    });
    if let Some(b) = single {
        set_auth(&mut workspace, &b.workspace.auth);
    }

    let mut out = Out {
        workspace_id,
        folders: Vec::new(),
        requests: Vec::new(),
        warnings: &mut warnings,
    };
    for (i, b) in bundles.iter().enumerate() {
        let root = if single.is_some() {
            None
        } else {
            let id = format!("fl_{}", b.workspace.id);
            let mut folder = json!({
                "model": "folder",
                "id": id,
                "workspaceId": out.workspace_id,
                "folderId": null,
                "name": b.workspace.name,
                "sortPriority": i as f64,
                "headers": headers(&b.workspace.headers),
            });
            set_auth(&mut folder, &b.workspace.auth);
            out.folders.push(folder);
            Some(id)
        };
        walk(b, None, root.as_deref(), &mut out);
    }
    let Out {
        workspace_id,
        folders,
        requests,
        ..
    } = out;

    let (globals, named) = split_envs(bundles);
    let base_id = format!("ev_{workspace_id}");
    let mut environments = vec![json!({
        "model": "environment",
        "id": base_id,
        "workspaceId": workspace_id,
        "name": "Global",
        "parentModel": "workspace",
        "parentId": null,
        "variables": globals
            .iter()
            .map(|v| json!({ "enabled": v.enabled, "name": v.key, "value": tpl(&v.value) }))
            .collect::<Vec<_>>(),
    })];
    environments.extend(named.into_iter().map(|(name, e)| {
        json!({
            "model": "environment",
            "id": format!("ev_{}_{}", e.workspace_id, e.id),
            "workspaceId": workspace_id,
            "name": name,
            "parentModel": "environment",
            "parentId": base_id,
            "variables": e
                .variables
                .iter()
                .map(|v| json!({ "enabled": v.enabled, "name": v.key, "value": tpl(&v.value) }))
                .collect::<Vec<_>>(),
        })
    }));

    let doc = json!({
        "yaakVersion": "2025.1.0",
        "yaakSchema": 4,
        "timestamp": now_iso(),
        "resources": {
            "workspaces": [workspace],
            "environments": environments,
            "folders": folders,
            "httpRequests": requests,
        },
    });
    Ok(ExportResult {
        content: serde_json::to_string_pretty(&doc)
            .map_err(|e| YukiError::Storage(e.to_string()))?,
        warnings,
    })
}

/// `parent` is the core folder scope; `yaak_parent` the Yaak `folderId` it maps to.
fn walk(b: &Bundle, parent: Option<&str>, yaak_parent: Option<&str>, out: &mut Out) {
    for node in children(b, parent) {
        match node {
            Node::Folder(f) => {
                let id = format!("fl_{}_{}", b.workspace.id, f.id);
                let mut folder = json!({
                    "model": "folder",
                    "id": id,
                    "workspaceId": out.workspace_id,
                    "folderId": yaak_parent,
                    "name": f.name,
                    "sortPriority": f.order,
                    "headers": headers(&f.headers),
                });
                let auth = match &f.auth {
                    AuthConfig::Inherit {
                        from: InheritSource::Workspace,
                    } => effective_inherited_auth(
                        b,
                        f.folder_id.as_deref(),
                        &InheritSource::Workspace,
                        &f.auth,
                    ),
                    a => a,
                };
                set_auth(&mut folder, auth);
                if !f.variables.is_empty() {
                    out.warnings.push(format!(
                        "Folder '{}': folder variables aren't exported to Yaak.",
                        f.name
                    ));
                }
                out.folders.push(folder);
                walk(b, Some(&f.id), Some(&id), out);
            }
            Node::Http(r) => {
                let params: Vec<Value> = r
                    .parameters
                    .iter()
                    .map(|p| {
                        // Yaak marks path params with a leading `:`.
                        let name = if contains_path_parameter(&r.url, &p.name) {
                            format!(":{}", p.name)
                        } else {
                            p.name.clone()
                        };
                        json!({ "enabled": p.enabled, "name": name, "value": tpl(&p.value) })
                    })
                    .collect();
                let mut req = json!({
                    "model": "http_request",
                    "id": format!("rq_{}_{}", b.workspace.id, r.id),
                    "workspaceId": out.workspace_id,
                    "folderId": yaak_parent,
                    "name": r.name,
                    "method": r.method,
                    "url": tpl(&r.url),
                    "urlParameters": params,
                    "headers": request_headers(b, r),
                    "sortPriority": r.order,
                    "bodyType": null,
                    "body": {},
                });
                if let Some((ty, body)) = r.body.as_ref().and_then(body) {
                    req["bodyType"] = json!(ty);
                    req["body"] = body;
                }
                let auth = match &r.auth {
                    AuthConfig::Inherit {
                        from: InheritSource::Workspace,
                    } => effective_inherited_auth(
                        b,
                        r.folder_id.as_deref(),
                        &InheritSource::Workspace,
                        &r.auth,
                    ),
                    a => a,
                };
                set_auth(&mut req, auth);
                // Unlike a folder's, a request's `None` is an explicit opt-out of inheritance.
                if !matches!(auth, AuthConfig::Inherit { .. })
                    && req["authenticationType"].is_null()
                {
                    req["authenticationType"] = json!("none");
                }
                out.requests.push(req);
            }
        }
    }
}

/// Sets `authenticationType`/`authentication`; null type = inherit from parent.
/// Every core scheme has a Yaak auth plugin (`awsv4`, `digest`, `windows`, …).
fn set_auth(obj: &mut Value, a: &AuthConfig) {
    let (ty, auth) = auth_of(a).map_or((Value::Null, json!({})), |(t, v)| (json!(t), v));
    obj["authenticationType"] = ty;
    obj["authentication"] = auth;
}

fn auth_of(a: &AuthConfig) -> Option<(&'static str, Value)> {
    if !a.is_active() {
        return None;
    }
    Some(match a {
        AuthConfig::Bearer { token, .. } => ("bearer", json!({ "token": tpl(token) })),
        AuthConfig::Basic {
            username, password, ..
        } => (
            "basic",
            json!({ "username": tpl(username), "password": tpl(password) }),
        ),
        AuthConfig::ApiKey {
            key,
            value,
            location,
            ..
        } => (
            "apikey",
            json!({
                "key": tpl(key),
                "value": tpl(value),
                "location": if *location == ApiKeyLocation::Query { "query" } else { "header" },
            }),
        ),
        AuthConfig::OAuth2 {
            grant_type,
            auth_url,
            token_url,
            client_id,
            client_secret,
            scope,
            audience,
            client_auth,
            use_pkce,
            code_challenge_method,
            code_verifier,
            redirect_uri,
            state,
            username,
            password,
            use_external_browser,
            ..
        } => (
            "oauth2",
            json!({
                "grantType": match grant_type {
                    OAuth2Grant::ClientCredentials => "client_credentials",
                    OAuth2Grant::AuthorizationCode => "authorization_code",
                    OAuth2Grant::Password => "password",
                    OAuth2Grant::Implicit => "implicit",
                },
                "authorizationUrl": tpl(auth_url),
                "accessTokenUrl": tpl(token_url),
                "clientId": tpl(client_id),
                "clientSecret": tpl(client_secret),
                "scope": tpl(scope),
                "audience": tpl(audience),
                "redirectUri": tpl(redirect_uri),
                "state": tpl(state),
                "username": tpl(username),
                "password": tpl(password),
                // Yaak defaults to `body`; Yuki to a Basic header.
                "credentials": if *client_auth == OAuth2ClientAuth::RequestBody { "body" } else { "basic" },
                "usePkce": use_pkce,
                "pkceChallengeMethod": if *code_challenge_method == OAuth2PkceMethod::Plain { "plain" } else { "S256" },
                "pkceCodeChallenge": tpl(code_verifier),
                "useExternalBrowser": use_external_browser,
            }),
        ),
        AuthConfig::OAuth1 {
            consumer_key,
            consumer_secret,
            token,
            token_secret,
            signature_method,
            realm,
            private_key,
            callback,
            verifier,
            timestamp,
            nonce,
            version,
            ..
        } => (
            "oauth1",
            json!({
                "signatureMethod": match signature_method {
                    OAuth1Signature::HmacSha1 => "HMAC-SHA1",
                    OAuth1Signature::HmacSha256 => "HMAC-SHA256",
                    OAuth1Signature::HmacSha512 => "HMAC-SHA512",
                    OAuth1Signature::RsaSha1 => "RSA-SHA1",
                    OAuth1Signature::RsaSha256 => "RSA-SHA256",
                    OAuth1Signature::RsaSha512 => "RSA-SHA512",
                    OAuth1Signature::PlainText => "PLAINTEXT",
                },
                "consumerKey": tpl(consumer_key),
                "consumerSecret": tpl(consumer_secret),
                "tokenKey": tpl(token),
                "tokenSecret": tpl(token_secret),
                "privateKey": tpl(private_key),
                "realm": tpl(realm),
                "callback": tpl(callback),
                "verifier": tpl(verifier),
                "timestamp": tpl(timestamp),
                "nonce": tpl(nonce),
                "version": tpl(version),
            }),
        ),
        AuthConfig::AwsSigV4 {
            access_key,
            secret_key,
            session_token,
            region,
            service,
            ..
        } => (
            "awsv4",
            json!({
                "accessKeyId": tpl(access_key),
                "secretAccessKey": tpl(secret_key),
                "sessionToken": tpl(session_token),
                "region": tpl(region),
                "service": tpl(service),
            }),
        ),
        AuthConfig::Digest {
            username, password, ..
        } => (
            "digest",
            json!({ "username": tpl(username), "password": tpl(password) }),
        ),
        AuthConfig::Ntlm {
            username,
            password,
            domain,
            workstation,
            ..
        } => (
            "windows",
            json!({
                "username": tpl(username),
                "password": tpl(password),
                "domain": tpl(domain),
                "workstation": tpl(workstation),
            }),
        ),
        AuthConfig::None | AuthConfig::Inherit { .. } => return None,
    })
}

fn body(b: &RequestBody) -> Option<(&'static str, Value)> {
    let text = || json!({ "text": tpl(&b.text) });
    Some(match b.kind {
        BodyKind::None => return None,
        BodyKind::Json => ("application/json", text()),
        BodyKind::Xml => ("text/xml", text()),
        BodyKind::Html => ("text/html", text()),
        BodyKind::Text => ("text/plain", text()),
        BodyKind::FormUrlEncoded => ("application/x-www-form-urlencoded", form(b)),
        BodyKind::Multipart => ("multipart/form-data", form(b)),
        BodyKind::Binary => ("binary", json!({ "filePath": b.file_path })),
        BodyKind::Graphql => (
            "graphql",
            json!({
                "query": tpl(&b.text),
                "variables": b.graphql_variables.as_deref().map(tpl).unwrap_or_default(),
            }),
        ),
    })
}

fn form(b: &RequestBody) -> Value {
    let fields: Vec<Value> = b
        .fields
        .iter()
        .flatten()
        .map(|f| {
            let mut v = json!({ "enabled": f.enabled, "name": f.name });
            if f.is_file {
                v["file"] = json!(f.value);
            } else {
                v["value"] = json!(tpl(&f.value));
            }
            if let Some(ct) = &f.content_type {
                v["contentType"] = json!(ct);
            }
            v
        })
        .collect();
    json!({ "form": fields })
}

fn request_headers(b: &Bundle, r: &HttpRequest) -> Vec<Value> {
    let mut hs = headers(&r.headers);
    if let Some(ct) = implied_content_type(b, r) {
        hs.push(json!({ "enabled": true, "name": "Content-Type", "value": ct }));
    }
    hs
}

fn headers(params: &[RequestParameter]) -> Vec<Value> {
    params
        .iter()
        .map(|p| json!({ "enabled": p.enabled, "name": p.name, "value": tpl(&p.value) }))
        .collect()
}

/// `{{ x }}` → `${[ x ]}` (the parser's inverse, so a round trip is stable).
fn tpl(s: &str) -> Cow<'_, str> {
    rewrite_templates(s, |expr| format!("${{[ {expr} ]}}"))
}
