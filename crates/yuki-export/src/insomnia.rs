//! Core types → Insomnia v4 export, the shape `yuki-import`'s Insomnia parser
//! reads back. One bundle maps onto the Insomnia workspace; several become one
//! request_group each under a combined workspace. Insomnia has no workspace-level
//! auth or headers, so both are flattened into requests: inherited auth is
//! written explicitly and workspace headers are merged unless a folder/request
//! overrides them. Templates `{{ x }}` become `{{ _.x }}`.

use std::borrow::Cow;

use serde_json::{json, Map, Value};
use yuki_core::{
    now_iso, ApiKeyLocation, AuthConfig, BodyField, BodyKind, EnvironmentVariable, InheritSource,
    OAuth2Grant, RequestBody, RequestParameter, YukiError,
};

use crate::auth::oauth1_sig;
use crate::postman::{contains_path_parameter, effective_inherited_auth};
use crate::{
    children, http_auths, implied_content_type, rewrite_templates, split_envs,
    warn_content_type_overrides, warn_count, warn_skipped_items, Bundle, ExportResult, Node,
};

pub fn to_insomnia(bundles: &[Bundle]) -> Result<ExportResult, YukiError> {
    let mut warnings = Vec::new();
    warn_skipped_items(bundles, "Insomnia", &mut warnings);
    warn_content_type_overrides(bundles, &mut warnings);
    let disabled_vars = bundles
        .iter()
        .flat_map(|b| {
            b.environments
                .iter()
                .flat_map(|e| &e.variables)
                .chain(b.folders.iter().flat_map(|f| &f.variables))
        })
        .filter(|v| !v.enabled)
        .count();
    warn_count(
        &mut warnings,
        disabled_vars,
        "disabled variable(s) were exported as active: Insomnia environments have no per-variable toggle.",
    );
    let ntlm_workstations = http_auths(bundles)
        .filter(|a| a.is_active())
        .filter(|a| matches!(a, AuthConfig::Ntlm { workstation, .. } if !workstation.is_empty()))
        .count();
    warn_count(
        &mut warnings,
        ntlm_workstations,
        "NTLM config(s) set a workstation, which Insomnia doesn't support; it was dropped.",
    );

    let single = match bundles {
        [b] => Some(b),
        _ => None,
    };
    let workspace_id = single.map_or_else(
        || "wrk_yuki_export".to_string(),
        |b| format!("wrk_{}", b.workspace.id),
    );
    let mut resources = vec![json!({
        "_id": workspace_id,
        "_type": "workspace",
        "parentId": null,
        "name": single.map_or("Yuki Export", |b| b.workspace.name.as_str()),
        "scope": "collection",
    })];

    for (i, b) in bundles.iter().enumerate() {
        let root = if single.is_some() {
            workspace_id.clone()
        } else {
            let id = format!("fld_{}", b.workspace.id);
            resources.push(json!({
                "_id": id,
                "_type": "request_group",
                "parentId": workspace_id,
                "name": b.workspace.name,
                "metaSortKey": i as f64,
            }));
            id
        };
        walk(b, None, &root, &[], &mut resources);
    }

    let (globals, named) = split_envs(bundles);
    let base_id = format!("env_{workspace_id}");
    resources.push(json!({
        "_id": base_id,
        "_type": "environment",
        "parentId": workspace_id,
        "name": "Base Environment",
        "data": env_data(globals),
    }));
    for (name, e) in named {
        resources.push(json!({
            "_id": format!("env_{}_{}", e.workspace_id, e.id),
            "_type": "environment",
            "parentId": base_id,
            "name": name,
            "data": env_data(&e.variables),
        }));
    }

    let doc = json!({
        "_type": "export",
        "__export_format": 4,
        "__export_date": now_iso(),
        "__export_source": "yuki",
        "resources": resources,
    });
    Ok(ExportResult {
        content: serde_json::to_string_pretty(&doc)
            .map_err(|e| YukiError::Storage(e.to_string()))?,
        warnings,
    })
}

/// `shadowed` = header names (lowercase) set by ancestor folders, which beat
/// workspace headers in Yuki and so must not be re-added at request level.
fn walk(
    b: &Bundle,
    parent: Option<&str>,
    parent_id: &str,
    shadowed: &[String],
    out: &mut Vec<Value>,
) {
    for node in children(b, parent) {
        match node {
            Node::Folder(f) => {
                let id = format!("fld_{}_{}", b.workspace.id, f.id);
                let mut group = json!({
                    "_id": id,
                    "_type": "request_group",
                    "parentId": parent_id,
                    "name": f.name,
                    "metaSortKey": f.order,
                    "headers": headers(&f.headers),
                    "environment": env_data(&f.variables),
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
                if auth.is_active() {
                    group["authentication"] = auth_of(auth);
                }
                out.push(group);
                let mut shadowed = shadowed.to_vec();
                shadowed.extend(f.headers.iter().map(|h| h.name.to_ascii_lowercase()));
                walk(b, Some(&f.id), &id, &shadowed, out);
            }
            Node::Http(r) => {
                let (path, query): (Vec<_>, Vec<_>) = r
                    .parameters
                    .iter()
                    .partition(|p| contains_path_parameter(&r.url, &p.name));
                let mut hs = headers(&r.headers);
                hs.extend(
                    b.workspace
                        .headers
                        .iter()
                        .filter(|w| {
                            !shadowed.iter().any(|s| s.eq_ignore_ascii_case(&w.name))
                                && !r
                                    .headers
                                    .iter()
                                    .any(|h| h.name.eq_ignore_ascii_case(&w.name))
                        })
                        .map(header),
                );
                if let Some(ct) = implied_content_type(b, r) {
                    hs.push(json!({ "name": "Content-Type", "value": ct, "disabled": false }));
                }
                // Inherit is resolved here: the workspace link has no Insomnia home.
                let auth = match &r.auth {
                    AuthConfig::Inherit { from } => {
                        effective_inherited_auth(b, r.folder_id.as_deref(), from, &r.auth)
                    }
                    a => a,
                };
                out.push(json!({
                    "_id": format!("req_{}_{}", b.workspace.id, r.id),
                    "_type": "request",
                    "parentId": parent_id,
                    "name": r.name,
                    "method": r.method,
                    "url": tpl(&r.url),
                    "metaSortKey": r.order,
                    "headers": hs,
                    "parameters": query.into_iter().map(header).collect::<Vec<_>>(),
                    "pathParameters": path
                        .into_iter()
                        .map(|p| json!({ "name": p.name, "value": tpl(&p.value) }))
                        .collect::<Vec<_>>(),
                    "body": r.body.as_ref().map_or_else(|| json!({}), body),
                    "authentication": if matches!(auth, AuthConfig::Inherit { .. }) {
                        json!({})
                    } else {
                        auth_of(auth)
                    },
                }));
            }
        }
    }
}

fn auth_of(a: &AuthConfig) -> Value {
    let mut v = match a {
        AuthConfig::None | AuthConfig::Inherit { .. } => return json!({ "type": "none" }),
        AuthConfig::Bearer { token, .. } => json!({ "type": "bearer", "token": tpl(token) }),
        AuthConfig::Basic {
            username, password, ..
        } => json!({ "type": "basic", "username": tpl(username), "password": tpl(password) }),
        AuthConfig::ApiKey {
            key,
            value,
            location,
            ..
        } => json!({
            "type": "apikey",
            "key": tpl(key),
            "value": tpl(value),
            "addTo": if *location == ApiKeyLocation::Query { "queryParams" } else { "header" },
        }),
        AuthConfig::AwsSigV4 {
            access_key,
            secret_key,
            session_token,
            region,
            service,
            ..
        } => json!({
            "type": "iam",
            "accessKeyId": tpl(access_key),
            "secretAccessKey": tpl(secret_key),
            "sessionToken": tpl(session_token),
            "region": tpl(region),
            "service": tpl(service),
        }),
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
        } => json!({
            "type": "oauth1",
            "consumerKey": tpl(consumer_key),
            "consumerSecret": tpl(consumer_secret),
            "tokenKey": tpl(token),
            "tokenSecret": tpl(token_secret),
            "signatureMethod": oauth1_sig(signature_method),
            "realm": tpl(realm),
            "privateKey": private_key,
            "callback": tpl(callback),
            "verifier": tpl(verifier),
            "timestamp": tpl(timestamp),
            "nonce": tpl(nonce),
            "version": tpl(version),
        }),
        AuthConfig::OAuth2 {
            grant_type,
            auth_url,
            token_url,
            client_id,
            client_secret,
            scope,
            audience,
            use_pkce,
            redirect_uri,
            state,
            username,
            password,
            ..
        } => json!({
            "type": "oauth2",
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
            "usePkce": use_pkce,
            "redirectUrl": tpl(redirect_uri),
            "state": tpl(state),
            "username": tpl(username),
            "password": tpl(password),
        }),
        AuthConfig::Digest {
            username, password, ..
        } => json!({ "type": "digest", "username": tpl(username), "password": tpl(password) }),
        AuthConfig::Ntlm {
            username,
            password,
            domain,
            ..
        } => {
            // Insomnia has no domain field; `DOMAIN\user` is the form NTLM accepts.
            let user = if domain.is_empty() {
                tpl(username).into_owned()
            } else {
                format!("{}\\{}", tpl(domain), tpl(username))
            };
            json!({ "type": "ntlm", "username": user, "password": tpl(password) })
        }
    };
    if !a.is_enabled() {
        v["disabled"] = json!(true);
    }
    v
}

fn body(b: &RequestBody) -> Value {
    let text = |mime: &str| json!({ "mimeType": mime, "text": tpl(&b.text) });
    match b.kind {
        BodyKind::None => json!({}),
        BodyKind::Json => text("application/json"),
        BodyKind::Xml => text("application/xml"),
        BodyKind::Html => text("text/html"),
        BodyKind::Text => text("text/plain"),
        BodyKind::FormUrlEncoded => json!({
            "mimeType": "application/x-www-form-urlencoded",
            "params": b.fields.iter().flatten().map(field).collect::<Vec<_>>(),
        }),
        BodyKind::Multipart => json!({
            "mimeType": "multipart/form-data",
            "params": b.fields.iter().flatten().map(field).collect::<Vec<_>>(),
        }),
        BodyKind::Binary => json!({
            "mimeType": "application/octet-stream",
            "fileName": b.file_path.as_deref().unwrap_or(""),
        }),
        BodyKind::Graphql => json!({
            "mimeType": "application/graphql",
            "text": tpl(&b.graphql_payload()).into_owned(),
        }),
    }
}

fn field(f: &BodyField) -> Value {
    if f.is_file {
        json!({ "name": f.name, "type": "file", "fileName": f.value, "disabled": !f.enabled })
    } else {
        json!({ "name": f.name, "value": tpl(&f.value), "disabled": !f.enabled })
    }
}

fn headers(params: &[RequestParameter]) -> Vec<Value> {
    params.iter().map(header).collect()
}

fn header(p: &RequestParameter) -> Value {
    json!({ "name": p.name, "value": tpl(&p.value), "disabled": !p.enabled })
}

/// Insomnia env data has no per-variable toggle, so disabled variables are
/// exported as plain values.
fn env_data<'a>(vars: impl IntoIterator<Item = &'a EnvironmentVariable>) -> Map<String, Value> {
    vars.into_iter()
        .map(|v| (v.key.clone(), json!(tpl(&v.value))))
        .collect()
}

/// `{{ x }}` → `{{ _.x }}` for plain variable names; anything else (functions,
/// filters) is left as written.
fn tpl(s: &str) -> Cow<'_, str> {
    rewrite_templates(s, |expr| {
        if !expr.is_empty()
            && expr
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        {
            format!("{{{{ _.{expr} }}}}")
        } else {
            format!("{{{{ {expr} }}}}")
        }
    })
}
