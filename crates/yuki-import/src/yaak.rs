//! Yaak data-export JSON → IR. The export is `{ yaakSchema, resources: {
//! workspaces, folders, httpRequests, environments, … } }`; we rebuild the tree
//! by `folderId` (null = workspace root) and order siblings by `sortPriority`.
//! Yaak templating is `${[ var ]}` — converted to Voleeo's `{{ var }}` per string
//! field (a global replace is unsafe: the `]}` closer collides with JSON).
//! Auth follows Yaak: a missing `authenticationType` inherits from the folder /
//! workspace, any value (including `"none"`) is the item's own. Older schemas
//! (1–4) are read the way Yaak's own importer migrates them. gRPC/WebSocket
//! resources are skipped with a warning; the base environment becomes collection
//! variables, folder environments folder variables, every other environment a
//! named one.

use crate::convert::oauth2;
use crate::ir::*;
use crate::util::{name_of, parse_value, strip_query};
use crate::ImportError;
use serde_json::Value;
use std::collections::HashMap;
use yuki_core::{
    AuthConfig, OAuth1Location, OAuth1Signature, OAuth2ClientAuth, OAuth2Grant, OAuth2PkceMethod,
};

pub fn parse_yaak(content: &str) -> Result<ImportedCollection, ImportError> {
    let doc = parse_value(content)?;
    let Some(schema) = doc.get("yaakSchema") else {
        return Err(ImportError::Parse(
            "missing `yaakSchema` (not a Yaak export)".into(),
        ));
    };
    let resources = doc.get("resources");
    let workspaces = arr(resources, "workspaces");
    let folders = arr(resources, "folders");
    // Schema 1 called them `requests`.
    let requests = match arr(resources, "httpRequests") {
        [] => arr(resources, "requests"),
        r => r,
    };
    let workspace = workspaces.first();
    let mut envs = collect_environments(arr(resources, "environments"));
    // Schema ≤ 2 kept the base variables on the workspace itself.
    if let Some(w) = workspace {
        envs.base.extend(env_vars(w));
    }

    let mut warnings = Vec::new();
    let skipped = arr(resources, "grpcRequests").len() + arr(resources, "websocketRequests").len();
    if skipped > 0 {
        warnings.push(format!("{skipped} WebSocket/gRPC item(s) skipped"));
    }

    Ok(ImportedCollection {
        name: workspace
            .and_then(|w| w.get("name").and_then(Value::as_str))
            .unwrap_or("Imported API")
            .to_string(),
        version: Some(
            schema
                .as_i64()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "4".into()),
        ),
        variables: envs.base,
        environments: envs.named,
        root_auth: workspace.map(auth_of).unwrap_or_default(),
        root_headers: workspace.map(headers_of).unwrap_or_default(),
        items: build_children(folders, requests, None, &envs.folders),
        warnings,
    })
}

/// Folders + requests whose `folderId` matches `parent` (None = workspace root),
/// ordered by `sortPriority`.
fn build_children(
    folders: &[Value],
    requests: &[Value],
    parent: Option<&str>,
    folder_vars: &HashMap<String, Vec<ImportedVariable>>,
) -> Vec<ImportedItem> {
    enum Node<'a> {
        Folder(&'a Value),
        Request(&'a Value),
    }
    let mut nodes: Vec<(f64, Node)> = Vec::new();
    for f in folders {
        if folder_id(f) == parent {
            nodes.push((sort_key(f), Node::Folder(f)));
        }
    }
    for r in requests {
        if folder_id(r) == parent {
            nodes.push((sort_key(r), Node::Request(r)));
        }
    }
    nodes.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    nodes
        .into_iter()
        .map(|(_, n)| match n {
            Node::Folder(f) => ImportedItem::Folder(ImportedFolder {
                name: name_of(f),
                description: f
                    .get("description")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
                auth: auth_of(f),
                headers: headers_of(f),
                variables: id_of(f)
                    .and_then(|id| folder_vars.get(id))
                    .cloned()
                    .unwrap_or_default(),
                items: build_children(folders, requests, id_of(f), folder_vars),
            }),
            Node::Request(r) => ImportedItem::Request(build_request(r)),
        })
        .collect()
}

fn build_request(r: &Value) -> ImportedRequest {
    let url = tv(r, "url");
    let mut method = sv(r, "method").to_uppercase();
    if method.is_empty() {
        method = "GET".into();
    }
    let mut req = ImportedRequest {
        name: name_of(r),
        method,
        url: strip_query(&url).to_string(),
        path: url.clone(),
        ..Default::default()
    };

    if let Some(params) = r.get("urlParameters").and_then(Value::as_array) {
        for p in params {
            let name = p.get("name").and_then(Value::as_str).unwrap_or("");
            if name.is_empty() {
                continue;
            }
            let value = tv(p, "value");
            // A leading `:` marks a path parameter (the url already carries `:name`).
            if let Some(path_name) = name.strip_prefix(':') {
                req.path_params.push(ImportedParam {
                    name: path_name.to_string(),
                    value,
                    enabled: true,
                });
            } else {
                req.query.push(ImportedParam {
                    name: name.to_string(),
                    value,
                    enabled: enabled(p),
                });
            }
        }
    }
    req.headers = headers_of(r);
    req.body = body_to_body(r);
    req.auth = auth_of(r);
    req
}

fn body_to_body(r: &Value) -> Option<ImportedBody> {
    let bt = r.get("bodyType").and_then(Value::as_str)?;
    let body = r.get("body").unwrap_or(&Value::Null);
    let raw = |hint| {
        Some(ImportedBody::Raw {
            hint,
            text: tv(body, "text"),
        })
    };
    match bt {
        "" => None,
        "application/json" => raw(RawKind::Json),
        "application/x-www-form-urlencoded" => {
            Some(ImportedBody::FormUrlEncoded(form_fields(body)))
        }
        "multipart/form-data" => Some(ImportedBody::Multipart(form_fields(body))),
        "graphql" => Some(ImportedBody::GraphQl {
            query: tv(body, "query"),
            variables: body
                .get("variables")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(convert_template),
        }),
        "binary" => Some(ImportedBody::Binary(sv(body, "filePath"))),
        t if t.contains("xml") => raw(RawKind::Xml),
        t if t.contains("html") => raw(RawKind::Html),
        _ => raw(RawKind::Text),
    }
}

fn form_fields(body: &Value) -> Vec<ImportedField> {
    body.get("form")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|f| {
                    let name = f.get("name").and_then(Value::as_str)?;
                    let file = sv(f, "file");
                    Some(ImportedField {
                        name: name.to_string(),
                        enabled: enabled(f),
                        content_type: Some(sv(f, "contentType")).filter(|c| !c.is_empty()),
                        is_file: !file.is_empty(),
                        value: if file.is_empty() {
                            tv(f, "value")
                        } else {
                            file
                        },
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The auth a workspace/folder/request declares (see the module doc for the
/// inheritance rule). Keys are the Yaak auth plugins' argument names.
fn auth_of(v: &Value) -> ImportedAuth {
    let Some(kind) = v.get("authenticationType").and_then(Value::as_str) else {
        return ImportedAuth::Inherit;
    };
    let a = v.get("authentication").unwrap_or(&Value::Null);
    if a.get("disabled").and_then(Value::as_bool) == Some(true) {
        return ImportedAuth::None;
    }
    let get = |k: &str| tv(a, k);
    let flag = |k: &str| a.get(k).and_then(Value::as_bool).unwrap_or(false);
    match kind {
        "none" | "" => ImportedAuth::None,
        "basic" => ImportedAuth::Basic {
            username: get("username"),
            password: get("password"),
        },
        // The plugin's prefix defaults to "Bearer"; any other one only fits a raw header.
        "bearer" => match a.get("prefix").map(|_| get("prefix")) {
            Some(p) if !p.trim().eq_ignore_ascii_case("bearer") => ImportedAuth::ApiKey {
                key: "Authorization".into(),
                value: format!("{} {}", p.trim(), get("token")).trim().to_string(),
                in_header: true,
            },
            _ => ImportedAuth::Bearer {
                token: get("token"),
            },
        },
        "apikey" => {
            let key = get("key");
            ImportedAuth::ApiKey {
                key: if key.is_empty() {
                    "X-API-Key".into()
                } else {
                    key
                },
                value: get("value"),
                in_header: get("location") != "query",
            }
        }
        "oauth2" => {
            let grant = match get("grantType").as_str() {
                "authorization_code" => OAuth2Grant::AuthorizationCode,
                "password" => OAuth2Grant::Password,
                "implicit" => OAuth2Grant::Implicit,
                _ => OAuth2Grant::ClientCredentials,
            };
            let mut auth = oauth2(
                grant,
                get("authorizationUrl"),
                get("accessTokenUrl"),
                get("clientId"),
                get("clientSecret"),
                get("scope"),
            );
            if let AuthConfig::OAuth2 {
                username,
                password,
                redirect_uri,
                state,
                audience,
                client_auth,
                use_pkce,
                code_challenge_method,
                code_verifier,
                use_external_browser,
                ..
            } = &mut auth
            {
                *username = get("username");
                *password = get("password");
                *redirect_uri = get("redirectUri");
                *state = get("state");
                *audience = get("audience");
                // Yaak sends credentials in the body unless told "basic".
                *client_auth = if get("credentials") == "basic" {
                    OAuth2ClientAuth::BasicHeader
                } else {
                    OAuth2ClientAuth::RequestBody
                };
                *use_pkce = flag("usePkce");
                if get("pkceChallengeMethod") == "plain" {
                    *code_challenge_method = OAuth2PkceMethod::Plain;
                }
                *use_external_browser = flag("useExternalBrowser");
                *code_verifier = get("pkceCodeChallenge");
            }
            ImportedAuth::Core(auth)
        }
        "oauth1" => ImportedAuth::Core(AuthConfig::OAuth1 {
            consumer_key: get("consumerKey"),
            consumer_secret: get("consumerSecret"),
            consumer_secret_encrypted: false,
            token: get("tokenKey"),
            token_secret: get("tokenSecret"),
            token_secret_encrypted: false,
            signature_method: match get("signatureMethod").as_str() {
                "HMAC-SHA256" => OAuth1Signature::HmacSha256,
                "HMAC-SHA512" => OAuth1Signature::HmacSha512,
                "RSA-SHA1" => OAuth1Signature::RsaSha1,
                "RSA-SHA256" => OAuth1Signature::RsaSha256,
                "RSA-SHA512" => OAuth1Signature::RsaSha512,
                "PLAINTEXT" => OAuth1Signature::PlainText,
                _ => OAuth1Signature::HmacSha1,
            },
            realm: get("realm"),
            private_key: get("privateKey"),
            private_key_encrypted: false,
            params_location: OAuth1Location::Header,
            callback: get("callback"),
            verifier: get("verifier"),
            timestamp: get("timestamp"),
            nonce: get("nonce"),
            version: get("version"),
            enabled: true,
        }),
        "awsv4" => ImportedAuth::Core(AuthConfig::AwsSigV4 {
            access_key: get("accessKeyId"),
            secret_key: get("secretAccessKey"),
            secret_key_encrypted: false,
            session_token: get("sessionToken"),
            session_token_encrypted: false,
            region: get("region"),
            service: get("service"),
            enabled: true,
        }),
        "digest" => ImportedAuth::Core(AuthConfig::Digest {
            username: get("username"),
            password: get("password"),
            password_encrypted: false,
            enabled: true,
        }),
        "windows" => ImportedAuth::Core(AuthConfig::Ntlm {
            username: get("username"),
            password: get("password"),
            password_encrypted: false,
            domain: get("domain"),
            workstation: get("workstation"),
            enabled: true,
        }),
        other => ImportedAuth::Unsupported(other.to_string()),
    }
}

#[derive(Default)]
struct Envs {
    base: Vec<ImportedVariable>,
    named: Vec<ImportedEnvironment>,
    /// Folder id → that folder's environment variables.
    folders: HashMap<String, Vec<ImportedVariable>>,
}

/// Base (→ Global), folder and named environments. Schema 5 says which via
/// `parentModel`; schema 4 used `base: bool`, schema 3 `environmentId: null`
/// for the base one.
fn collect_environments(envs: &[Value]) -> Envs {
    let mut out = Envs::default();
    for e in envs {
        let vars = env_vars(e);
        let base = match e.get("parentModel").and_then(Value::as_str) {
            Some(model) => model == "workspace",
            None => match e.get("base").and_then(Value::as_bool) {
                Some(b) => b,
                None => e.get("environmentId").is_some_and(Value::is_null),
            },
        };
        if base {
            out.base.extend(vars);
        } else if e.get("parentModel").and_then(Value::as_str) == Some("folder") {
            if let Some(id) = e.get("parentId").and_then(Value::as_str) {
                out.folders.entry(id.to_string()).or_default().extend(vars);
            }
        } else {
            out.named.push(ImportedEnvironment {
                name: name_of(e),
                variables: vars,
            });
        }
    }
    out
}

fn env_vars(e: &Value) -> Vec<ImportedVariable> {
    e.get("variables")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| {
                    let key = v.get("name").and_then(Value::as_str)?;
                    if key.is_empty() {
                        return None;
                    }
                    Some(ImportedVariable {
                        key: key.to_string(),
                        value: tv(v, "value"),
                        enabled: enabled(v),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn headers_of(v: &Value) -> Vec<ImportedParam> {
    v.get("headers")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|h| {
                    let name = h.get("name").and_then(Value::as_str)?;
                    if name.is_empty() {
                        return None;
                    }
                    Some(ImportedParam {
                        name: name.to_string(),
                        value: tv(h, "value"),
                        enabled: enabled(h),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Convert Yaak `${[ var ]}` tokens to Voleeo `{{ var }}`. Per-field and scoped to
/// the `${[`…`]}` span so a stray `]}` in JSON-like values is never touched.
fn convert_template(input: &str) -> String {
    if !input.contains("${[") {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("${[") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 3..];
        let Some(end) = after.find("]}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        out.push_str("{{ ");
        out.push_str(after[..end].trim());
        out.push_str(" }}");
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

fn arr<'a>(resources: Option<&'a Value>, key: &str) -> &'a [Value] {
    resources
        .and_then(|r| r.get(key))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn folder_id(v: &Value) -> Option<&str> {
    v.get("folderId").and_then(Value::as_str)
}
fn id_of(v: &Value) -> Option<&str> {
    v.get("id").and_then(Value::as_str)
}
fn sort_key(v: &Value) -> f64 {
    v.get("sortPriority").and_then(Value::as_f64).unwrap_or(0.0)
}
fn enabled(v: &Value) -> bool {
    v.get("enabled").and_then(Value::as_bool) != Some(false)
}
fn sv(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}
fn tv(v: &Value, key: &str) -> String {
    convert_template(&sv(v, key))
}
