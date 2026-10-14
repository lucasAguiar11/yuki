//! Export a set of Voleeo workspaces. Two targets:
//! - **Voleeo Bundle** (`to_voleeo`): one self-contained, lossless YAML of the
//!   native core types — re-importable into Voleeo with full fidelity.
//! - **Postman Collection v2.1** (`to_postman`): a portable HTTP collection, with
//!   gRPC/WebSocket emitted as companion `.proto`/AsyncAPI files by the caller.
//!
//! Pure — no Tauri, storage, or crypto. The command layer decrypts secrets and
//! assembles the bundles; here values are already plaintext. We map **core types
//! directly**, never through `yuki-import`'s IR (which is lossy on auth and
//! drops WS/gRPC).

mod asyncapi;
mod auth;
mod insomnia;
mod postman;
mod voleeo;
mod yaak;

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
};

use yuki_core::{
    ApiFolder, AuthConfig, BodyKind, Environment, EnvironmentKind, EnvironmentVariable,
    GrpcRequest, HttpRequest, RequestParameter, Workspace, WsConnection,
};

pub use asyncapi::to_asyncapi;
pub use insomnia::to_insomnia;
pub use postman::{postman_environments, to_postman, to_postman_with_roots};
pub use voleeo::to_voleeo;
pub use yaak::to_yaak;

/// A file the caller should write under a derived name (the command layer slugs
/// `name` into the final filename). Used for Postman environment exports.
pub struct NamedFile {
    pub name: String,
    pub content: String,
}

/// One workspace's fully-loaded, already-decrypted contents.
pub struct Bundle {
    pub workspace: Workspace,
    pub folders: Vec<ApiFolder>,
    pub requests: Vec<HttpRequest>,
    pub ws: Vec<WsConnection>,
    pub grpc: Vec<GrpcRequest>,
    pub environments: Vec<Environment>,
}

impl Bundle {
    /// Blank every secret: auth secret fields at every scope and the value of
    /// each `encrypted` variable (environments + folder variables).
    pub fn strip_secrets(&mut self) {
        let auths = std::iter::once(&mut self.workspace.auth)
            .chain(self.folders.iter_mut().map(|f| &mut f.auth))
            .chain(self.requests.iter_mut().map(|r| &mut r.auth))
            .chain(self.ws.iter_mut().map(|w| &mut w.auth))
            .chain(self.grpc.iter_mut().map(|g| &mut g.auth));
        for auth in auths {
            for (secret, _) in auth.secret_fields_mut() {
                secret.clear();
            }
        }
        let vars = self
            .environments
            .iter_mut()
            .flat_map(|e| e.variables.iter_mut())
            .chain(self.folders.iter_mut().flat_map(|f| f.variables.iter_mut()));
        for var in vars.filter(|v| v.encrypted) {
            var.value.clear();
        }
    }
}

impl Bundle {
    /// Keep selected folder subtrees plus ancestor metadata needed for inheritance.
    /// Returns `false` when any selected folder is not part of the workspace.
    pub fn filter_to_folders(&mut self, folder_ids: &[&str]) -> bool {
        if folder_ids.is_empty() {
            return true;
        }

        let parents_by_id: HashMap<&str, Option<&str>> = self
            .folders
            .iter()
            .map(|folder| (folder.id.as_str(), folder.folder_id.as_deref()))
            .collect();
        if folder_ids.iter().any(|id| !parents_by_id.contains_key(id)) {
            return false;
        }

        let mut children_by_parent: HashMap<&str, Vec<&str>> = HashMap::new();
        for folder in &self.folders {
            if let Some(parent) = folder.folder_id.as_deref() {
                children_by_parent
                    .entry(parent)
                    .or_default()
                    .push(folder.id.as_str());
            }
        }

        let mut included: HashSet<String> = folder_ids.iter().map(|id| (*id).to_string()).collect();
        let mut pending = folder_ids.to_vec();
        while let Some(parent) = pending.pop() {
            for &child in children_by_parent.get(parent).into_iter().flatten() {
                if included.insert(child.to_string()) {
                    pending.push(child);
                }
            }
        }

        let mut folders_to_keep = included.clone();
        for root_id in folder_ids {
            let mut parent = parents_by_id.get(root_id).copied().flatten();
            let mut seen = HashSet::new();
            while let Some(parent_id) = parent {
                if !seen.insert(parent_id) {
                    break;
                }
                folders_to_keep.insert(parent_id.to_string());
                parent = parents_by_id.get(parent_id).copied().flatten();
            }
        }

        self.folders.retain(|f| folders_to_keep.contains(&f.id));
        self.requests.retain(|r| {
            r.folder_id
                .as_deref()
                .is_some_and(|id| included.contains(id))
        });
        self.ws.retain(|w| {
            w.folder_id
                .as_deref()
                .is_some_and(|id| included.contains(id))
        });
        self.grpc.retain(|g| {
            g.folder_id
                .as_deref()
                .is_some_and(|id| included.contains(id))
        });
        true
    }
}

/// Remove whitespace around template expressions for consumers that require the
/// compact `{{name}}` form. Function arguments inside the expression are kept.
pub(crate) fn normalize_templates(input: &str) -> Cow<'_, str> {
    rewrite_templates(input, |expr| format!("{{{{{expr}}}}}"))
}

/// Rewrite every `{{ expr }}` span via `wrap(trimmed expr)`; text outside spans
/// (and an unclosed `{{`) is kept verbatim.
pub(crate) fn rewrite_templates(input: &str, wrap: impl Fn(&str) -> String) -> Cow<'_, str> {
    if !input.contains("{{") {
        return Cow::Borrowed(input);
    }

    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let expression = &rest[start + 2..];
        let Some(end) = expression.find("}}") else {
            out.push_str(&rest[start..]);
            return Cow::Owned(out);
        };
        out.push_str(&wrap(expression[..end].trim()));
        rest = &expression[end + 2..];
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// Serialized output plus any non-fatal warnings (skipped/lossy items).
pub struct ExportResult {
    pub content: String,
    pub warnings: Vec<String>,
}

/// A node under one folder scope, kept in `order` so siblings interleave the way
/// the user arranged them. Only folders + HTTP requests form the collection tree;
/// gRPC/WS are exported to their own formats straight off the `Bundle`.
pub(crate) enum Node<'a> {
    Folder(&'a ApiFolder),
    Http(&'a HttpRequest),
}

/// Direct children of `parent` (`None` = workspace root), sorted by `order`.
pub(crate) fn children<'a>(b: &'a Bundle, parent: Option<&str>) -> Vec<Node<'a>> {
    let mut out: Vec<(f64, Node<'a>)> = Vec::new();
    for f in &b.folders {
        if f.folder_id.as_deref() == parent {
            out.push((f.order, Node::Folder(f)));
        }
    }
    for r in &b.requests {
        if r.folder_id.as_deref() == parent {
            out.push((r.order, Node::Http(r)));
        }
    }
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    out.into_iter().map(|(_, n)| n).collect()
}

/// Global variables of every bundle (last wins on a key clash) plus the named
/// environments with display names, workspace-prefixed when bundles are combined.
pub(crate) fn split_envs(
    bundles: &[Bundle],
) -> (Vec<&EnvironmentVariable>, Vec<(String, &Environment)>) {
    let mut globals: Vec<&EnvironmentVariable> = Vec::new();
    let mut named = Vec::new();
    for b in bundles {
        for e in &b.environments {
            if e.kind == EnvironmentKind::Global {
                globals.extend(&e.variables);
            } else if bundles.len() > 1 {
                named.push((format!("{} / {}", b.workspace.name, e.name), e));
            } else {
                named.push((e.name.clone(), e));
            }
        }
    }
    let mut seen = HashSet::new();
    globals.reverse();
    globals.retain(|v| seen.insert(v.key.as_str()));
    globals.reverse();
    (globals, named)
}

/// WebSocket/gRPC items have no place in HTTP-collection formats — say so.
pub(crate) fn warn_skipped_items(bundles: &[Bundle], target: &str, warnings: &mut Vec<String>) {
    for b in bundles {
        if !b.ws.is_empty() {
            warnings.push(format!(
                "{} WebSocket connection(s) in '{}' were skipped: {target} export carries HTTP requests only.",
                b.ws.len(),
                b.workspace.name
            ));
        }
        if !b.grpc.is_empty() {
            warnings.push(format!(
                "{} gRPC request(s) in '{}' were skipped: {target} export carries HTTP requests only.",
                b.grpc.len(),
                b.workspace.name
            ));
        }
    }
}

/// Auth configs that reach an HTTP collection: workspace, folders, requests.
pub(crate) fn http_auths(bundles: &[Bundle]) -> impl Iterator<Item = &AuthConfig> {
    bundles.iter().flat_map(|b| {
        std::iter::once(&b.workspace.auth)
            .chain(b.folders.iter().map(|f| &f.auth))
            .chain(b.requests.iter().map(|r| &r.auth))
    })
}

/// Push `"{count} {what}"` when `count > 0` — one warning per lossy kind.
pub(crate) fn warn_count(warnings: &mut Vec<String>, count: usize, what: &str) {
    if count > 0 {
        warnings.push(format!("{count} {what}"));
    }
}

/// A Content-Type override only drives the wire header for binary bodies in
/// yuki-http; elsewhere the exported header may differ from what Yuki sends.
pub(crate) fn warn_content_type_overrides(bundles: &[Bundle], warnings: &mut Vec<String>) {
    let count = bundles
        .iter()
        .flat_map(|b| &b.requests)
        .filter_map(|r| r.body.as_ref())
        .filter(|body| {
            !matches!(body.kind, BodyKind::None | BodyKind::Binary)
                && body
                    .content_type
                    .as_deref()
                    .is_some_and(|c| !c.trim().is_empty())
        })
        .count();
    warn_count(
        warnings,
        count,
        "request(s) carry a Content-Type override on a non-binary body; the exported Content-Type may differ from what Yuki sends.",
    );
}

/// The Content-Type yuki-http adds at send time when no enabled header in scope
/// (workspace → folders → request) sets one. Stored requests usually lack it, so
/// tools that send headers verbatim need it written out. Multipart is left to
/// the target tool (it owns the boundary).
pub(crate) fn implied_content_type(b: &Bundle, r: &HttpRequest) -> Option<String> {
    let body = r.body.as_ref()?;
    let mime = match body.kind {
        BodyKind::None | BodyKind::Multipart => return None,
        _ if body.text.is_empty()
            && matches!(
                body.kind,
                BodyKind::Json
                    | BodyKind::Xml
                    | BodyKind::Text
                    | BodyKind::Html
                    | BodyKind::Graphql
            ) =>
        {
            return None
        }
        BodyKind::Json | BodyKind::Graphql => "application/json",
        BodyKind::Xml => "application/xml",
        BodyKind::Html => "text/html",
        BodyKind::Text => "text/plain",
        BodyKind::FormUrlEncoded => "application/x-www-form-urlencoded",
        BodyKind::Binary => "application/octet-stream",
    };

    let mut scopes: Vec<&[RequestParameter]> = vec![&b.workspace.headers, &r.headers];
    let mut parent = r.folder_id.as_deref();
    let mut seen = HashSet::new();
    while let Some(id) = parent.filter(|id| seen.insert(*id)) {
        let Some(f) = b.folders.iter().find(|f| f.id == id) else {
            break;
        };
        scopes.push(&f.headers);
        parent = f.folder_id.as_deref();
    }
    let set = scopes
        .iter()
        .flat_map(|s| s.iter())
        .any(|h| h.enabled && h.name.trim().eq_ignore_ascii_case("content-type"));
    if set {
        return None;
    }
    Some(
        body.content_type
            .as_deref()
            .filter(|c| !c.trim().is_empty())
            .unwrap_or(mime)
            .to_string(),
    )
}
