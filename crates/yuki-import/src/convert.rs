use crate::ir::*;
use std::collections::{BTreeMap, HashSet};
use yuki_core::{
    new_id, now_iso, ApiFolder, ApiKeyLocation, AuthConfig, BodyField, BodyKind, Environment,
    EnvironmentKind, EnvironmentVariable, GrpcRequest, HttpRequest, OAuth2ClientAuth, OAuth2Grant,
    OAuth2PkceMethod, RequestBody, RequestParameter, WsConnection,
};

/// Fully-built structs ready to persist. Folders come out flattened (auth `None`,
/// no headers) because the send pipeline ignores `AuthConfig::Inherit` and
/// folder/workspace headers: each request carries its effective auth and merged
/// headers instead.
#[derive(Debug, Default)]
pub struct ConvertedPlan {
    pub folders: Vec<ApiFolder>,
    pub requests: Vec<HttpRequest>,
    /// Collection/base variables → the workspace's Global environment.
    pub variables: Vec<EnvironmentVariable>,
    /// Named environments (fresh ids, `workspace_id` set).
    pub environments: Vec<Environment>,
    /// Voleeo bundles only.
    pub websockets: Vec<WsConnection>,
    /// Voleeo bundles only.
    pub grpc: Vec<GrpcRequest>,
    pub warnings: Vec<String>,
}

/// Build the persistence plan. `order_base` seeds `order` (new workspaces use
/// `timestamp_millis`; existing ones use `max_sibling_order + 1` to append).
pub fn build_plan(workspace_id: &str, order_base: f64, col: &ImportedCollection) -> ConvertedPlan {
    let mut plan = ConvertedPlan {
        variables: col.variables.iter().map(to_env_var).collect(),
        environments: col
            .environments
            .iter()
            .map(|e| {
                let vars = e.variables.iter().map(to_env_var).collect();
                named_env(workspace_id, &e.name, vars)
            })
            .collect(),
        warnings: col.warnings.clone(),
        ..Default::default()
    };
    let root_auth = resolve(&col.root_auth, &AuthConfig::None);
    let root_headers = to_params(&col.root_headers);
    let mut order = order_base;
    convert_items(
        workspace_id,
        &col.items,
        None,
        (&root_auth, &root_headers),
        &mut order,
        &mut plan,
    );
    let mut unsupported = BTreeMap::new();
    count_unsupported(&col.root_auth, &col.items, &mut unsupported);
    for (kind, n) in unsupported {
        plan.warnings.push(format!(
            "`{kind}` auth ({n}×) isn't supported by Yuki; imported without auth."
        ));
    }
    plan
}

fn count_unsupported(
    auth: &ImportedAuth,
    items: &[ImportedItem],
    out: &mut BTreeMap<String, usize>,
) {
    if let ImportedAuth::Unsupported(kind) = auth {
        *out.entry(kind.clone()).or_default() += 1;
    }
    for item in items {
        match item {
            ImportedItem::Folder(f) => count_unsupported(&f.auth, &f.items, out),
            ImportedItem::Request(r) => count_unsupported(&r.auth, &[], out),
        }
    }
}

/// `scope` = (auth, headers) in effect for these items, already merged from root
/// and every enclosing folder.
fn convert_items(
    ws: &str,
    items: &[ImportedItem],
    parent: Option<&str>,
    scope: (&AuthConfig, &[RequestParameter]),
    order: &mut f64,
    plan: &mut ConvertedPlan,
) {
    for item in items {
        let this_order = *order;
        *order += 1.0;
        match item {
            ImportedItem::Folder(f) => {
                let mut folder = new_folder(ws, parent, &f.name, this_order);
                let id = folder.id.clone();
                folder.variables = f.variables.iter().map(to_env_var).collect();
                plan.folders.push(folder);
                let auth = resolve(&f.auth, scope.0);
                let headers = merge_headers(scope.1, &to_params(&f.headers));
                convert_items(ws, &f.items, Some(&id), (&auth, &headers), order, plan);
            }
            ImportedItem::Request(r) => {
                let now = now_iso();
                let mut parameters = to_params(&r.path_params);
                parameters.extend(to_params(&r.query));
                plan.requests.push(HttpRequest {
                    id: new_id(),
                    request_type: "api".to_string(),
                    model: "http_request".to_string(),
                    workspace_id: ws.to_string(),
                    folder_id: parent.map(str::to_string),
                    method: r.method.clone(),
                    name: r.name.clone(),
                    url: r.url.clone(),
                    parameters,
                    headers: merge_headers(scope.1, &to_params(&r.headers)),
                    body: to_body(r.body.as_ref()),
                    auth: resolve(&r.auth, scope.0),
                    order: this_order,
                    created_at: now.clone(),
                    updated_at: now,
                });
            }
        }
    }
}

/// A flattened folder: no auth/headers (those live on its requests).
pub(crate) fn new_folder(ws: &str, parent: Option<&str>, name: &str, order: f64) -> ApiFolder {
    let now = now_iso();
    ApiFolder {
        id: new_id(),
        folder_type: "api".to_string(),
        model: "folder".to_string(),
        workspace_id: ws.to_string(),
        folder_id: parent.map(str::to_string),
        name: name.to_string(),
        headers: Vec::new(),
        auth: AuthConfig::None,
        variables: Vec::new(),
        color: None,
        order,
        created_at: now.clone(),
        updated_at: now,
    }
}

/// Auth a scope hands to its children: its own when declared, else the outer one.
pub(crate) fn scope_auth(own: &AuthConfig, outer: &AuthConfig) -> AuthConfig {
    match own {
        AuthConfig::None | AuthConfig::Inherit { .. } => outer.clone(),
        declared => declared.clone(),
    }
}

/// A request's effective auth: `Inherit` resolves to the enclosing scope's; an
/// explicit `None` stays none.
pub(crate) fn request_auth(own: &AuthConfig, outer: &AuthConfig) -> AuthConfig {
    match own {
        AuthConfig::Inherit { .. } => outer.clone(),
        declared => declared.clone(),
    }
}

/// Outer headers, then inner ones; an inner header replaces every outer one with
/// the same name (case-insensitive). Fresh row ids — copies must not share them.
pub(crate) fn merge_headers(
    outer: &[RequestParameter],
    inner: &[RequestParameter],
) -> Vec<RequestParameter> {
    outer
        .iter()
        .filter(|o| !inner.iter().any(|i| i.name.eq_ignore_ascii_case(&o.name)))
        .chain(inner)
        .map(|p| RequestParameter {
            id: new_id(),
            ..p.clone()
        })
        .collect()
}

/// A personal (non-shared, uncolored) environment in `ws`.
fn named_env(ws: &str, name: &str, variables: Vec<EnvironmentVariable>) -> Environment {
    let now = now_iso();
    Environment {
        id: new_id(),
        workspace_id: ws.to_string(),
        kind: EnvironmentKind::Personal,
        name: name.to_string(),
        color: String::new(),
        shared: false,
        variables,
        created_at: now.clone(),
        updated_at: now,
    }
}

fn to_env_var(v: &ImportedVariable) -> EnvironmentVariable {
    EnvironmentVariable {
        key: v.key.clone(),
        value: v.value.clone(),
        encrypted: false,
        enabled: v.enabled,
    }
}

fn to_params(params: &[ImportedParam]) -> Vec<RequestParameter> {
    params
        .iter()
        .map(|p| RequestParameter {
            id: new_id(),
            name: p.name.clone(),
            value: p.value.clone(),
            enabled: p.enabled,
        })
        .collect()
}

fn to_body(body: Option<&ImportedBody>) -> Option<RequestBody> {
    let body = body?;
    let built = match body {
        ImportedBody::Raw { hint, text } => RequestBody {
            kind: match hint {
                RawKind::Json => BodyKind::Json,
                RawKind::Xml => BodyKind::Xml,
                RawKind::Text => BodyKind::Text,
                RawKind::Html => BodyKind::Html,
            },
            text: text.clone(),
            ..Default::default()
        },
        ImportedBody::FormUrlEncoded(fields) => RequestBody {
            kind: BodyKind::FormUrlEncoded,
            fields: Some(to_fields(fields)),
            ..Default::default()
        },
        ImportedBody::Multipart(fields) => RequestBody {
            kind: BodyKind::Multipart,
            fields: Some(to_fields(fields)),
            ..Default::default()
        },
        ImportedBody::Binary(path) => RequestBody {
            kind: BodyKind::Binary,
            file_path: (!path.is_empty()).then(|| path.clone()),
            ..Default::default()
        },
        ImportedBody::GraphQl { query, variables } => RequestBody {
            kind: BodyKind::Graphql,
            text: query.clone(),
            graphql_variables: variables.clone(),
            ..Default::default()
        },
    };
    Some(built)
}

fn to_fields(fields: &[ImportedField]) -> Vec<BodyField> {
    fields
        .iter()
        .map(|f| BodyField {
            id: new_id(),
            name: f.name.clone(),
            value: f.value.clone(),
            enabled: f.enabled,
            is_file: f.is_file,
            content_type: f.content_type.clone(),
        })
        .collect()
}

/// `own` resolved against the enclosing scope's effective auth.
fn resolve(own: &ImportedAuth, outer: &AuthConfig) -> AuthConfig {
    match own {
        ImportedAuth::Inherit => outer.clone(),
        ImportedAuth::None | ImportedAuth::Unsupported(_) => AuthConfig::None,
        ImportedAuth::Core(a) => a.clone(),
        ImportedAuth::Bearer { token } => AuthConfig::Bearer {
            token: token.clone(),
            token_encrypted: false,
            enabled: true,
        },
        ImportedAuth::Basic { username, password } => AuthConfig::Basic {
            username: username.clone(),
            password: password.clone(),
            password_encrypted: false,
            enabled: true,
        },
        ImportedAuth::ApiKey {
            key,
            value,
            in_header,
        } => AuthConfig::ApiKey {
            key: key.clone(),
            value: value.clone(),
            location: if *in_header {
                ApiKeyLocation::Header
            } else {
                ApiKeyLocation::Query
            },
            value_encrypted: false,
            enabled: true,
        },
    }
}

/// OAuth2 with Yuki's defaults (PKCE S256, client auth in a Basic header);
/// parsers set the remaining fields through `if let AuthConfig::OAuth2 {..}`.
pub(crate) fn oauth2(
    grant_type: OAuth2Grant,
    auth_url: String,
    token_url: String,
    client_id: String,
    client_secret: String,
    scope: String,
) -> AuthConfig {
    AuthConfig::OAuth2 {
        grant_type,
        auth_url,
        token_url,
        client_id,
        client_secret,
        client_secret_encrypted: false,
        scope,
        audience: String::new(),
        client_auth: OAuth2ClientAuth::BasicHeader,
        use_pkce: true,
        code_challenge_method: OAuth2PkceMethod::S256,
        code_verifier: String::new(),
        redirect_uri: String::new(),
        state: String::new(),
        username: String::new(),
        password: String::new(),
        password_encrypted: false,
        use_external_browser: false,
        enabled: true,
    }
}

// ── Preview tree + selection filter (shared positional-id scheme) ────────────
//
// A node's id is its index path from the root, joined by `.` (`"0"`, `"0.1"`,
// `"2"`). Parsing is deterministic, so the same id addresses the same node on
// the preview pass and the commit pass — selection round-trips without server
// state.

/// One preview node the UI renders as a checkbox tree.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportNode {
    pub id: String,
    /// `"folder"` | `"request"`.
    pub kind: String,
    pub name: String,
    /// Request HTTP method (folders have none).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Request source path (`/pet/{petId}`); folders have none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Folder/tag description; requests have none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub children: Vec<ImportNode>,
}

pub fn preview_nodes(items: &[ImportedItem]) -> Vec<ImportNode> {
    build_nodes(items, "")
}

fn build_nodes(items: &[ImportedItem], prefix: &str) -> Vec<ImportNode> {
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let id = format!("{prefix}{i}");
            match item {
                ImportedItem::Folder(f) => ImportNode {
                    children: build_nodes(&f.items, &format!("{id}.")),
                    id,
                    kind: "folder".to_string(),
                    name: f.name.clone(),
                    method: None,
                    path: None,
                    description: f.description.clone(),
                },
                ImportedItem::Request(r) => ImportNode {
                    id,
                    kind: "request".to_string(),
                    name: r.name.clone(),
                    method: Some(r.method.clone()),
                    path: Some(r.path.clone()),
                    description: None,
                    children: Vec::new(),
                },
            }
        })
        .collect()
}

/// Keep only the selected nodes. A request survives when its id is selected; a
/// folder survives when it has surviving children (or is an explicitly-selected
/// empty folder). `selected` ids use the `preview_nodes` scheme.
pub fn filter_items(items: &[ImportedItem], selected: &HashSet<String>) -> Vec<ImportedItem> {
    filter_inner(items, selected, "")
}

fn filter_inner(
    items: &[ImportedItem],
    selected: &HashSet<String>,
    prefix: &str,
) -> Vec<ImportedItem> {
    let mut out = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let id = format!("{prefix}{i}");
        match item {
            ImportedItem::Request(_) => {
                if selected.contains(&id) {
                    out.push(item.clone());
                }
            }
            ImportedItem::Folder(f) => {
                let kids = filter_inner(&f.items, selected, &format!("{id}."));
                if !kids.is_empty() || (f.items.is_empty() && selected.contains(&id)) {
                    let mut nf = f.clone();
                    nf.items = kids;
                    out.push(ImportedItem::Folder(nf));
                }
            }
        }
    }
    out
}
