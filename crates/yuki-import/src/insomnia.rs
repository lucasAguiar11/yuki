//! Insomnia export → IR. v5 (`collection.insomnia.rest/5.0` YAML) nests its tree
//! and is handed to `insomnia_v5`; v4 resources are a flat array linked by
//! `parentId` — we reconstruct the tree (workspace → request_group* → request)
//! and order siblings by `metaSortKey`. Insomnia templating `{{ _.var }}` maps to
//! Voleeo's `{{ var }}` — done once as a textual normalize before parsing (the
//! `_.` prefix only ever appears inside these tags). `{% … %}` function tags
//! aren't supported and are left as text with a warning. Absent or disabled
//! auth inherits the nearest folder's (Insomnia's `getOrInheritAuthentication`).

mod auth;
mod notes;

use crate::ir::*;
use crate::util::{disabled, name_of, parse_value, strip_query};
use crate::ImportError;
pub(crate) use auth::to_auth;
pub(crate) use notes::Notes;
use serde_json::Value;
use std::collections::HashSet;

/// v5 file types that carry a `collection` and/or `environments`.
const V5_TYPES: [&str; 3] = [
    crate::insomnia_v5::V5_TYPE,
    "spec.insomnia.rest/5.0",
    "environment.insomnia.rest/5.0",
];

pub fn parse_insomnia(content: &str) -> Result<ImportedCollection, ImportError> {
    // `{{ _.x }}` → `{{ x }}`. Safe on the raw JSON: `_.` only appears in tags.
    let normalized = normalize_brackets(&content.replace("{{ _.", "{{ ").replace("{{_.", "{{"));
    let doc = parse_value(&normalized)?;

    let mut warnings = Vec::new();
    if content.contains("{%") || has_template_filter(&normalized) {
        warnings.push(
            "Some Insomnia template tags ({% … %} or `|` filters) aren't supported and were \
             left as text — adjust them after import."
                .into(),
        );
    }
    if doc
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|t| V5_TYPES.contains(&t))
    {
        return Ok(crate::insomnia_v5::parse(&doc, warnings));
    }

    let resources = doc
        .get("resources")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ImportError::Parse("missing `resources` array (not an Insomnia export)".into())
        })?;

    let workspace = resources.iter().find(|r| type_of(r) == "workspace");
    let root_id = workspace
        .map(id_of)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            // No workspace resource: the root is whatever parent isn't itself a resource.
            let ids: HashSet<&str> = resources
                .iter()
                .map(id_of)
                .filter(|s| !s.is_empty())
                .collect();
            resources
                .iter()
                .filter(|r| matches!(type_of(r), "request" | "request_group"))
                .map(parent_of)
                .find(|p| !p.is_empty() && !ids.contains(p))
                .unwrap_or("")
        });

    let mut notes = Notes::default();
    for r in resources {
        match type_of(r) {
            "grpc_request" | "websocket_request" | "socketio_request" => notes.skipped += 1,
            "request" | "request_group" => notes.script(r),
            _ => {}
        }
    }
    let (variables, environments) = collect_environments(resources, root_id, &mut notes);
    let items = build_children(resources, root_id, &mut notes, &mut HashSet::new());
    notes.warn(&mut warnings);

    Ok(ImportedCollection {
        name: workspace
            .and_then(|w| w.get("name").and_then(Value::as_str))
            .unwrap_or("Imported API")
            .to_string(),
        version: Some("4".into()),
        variables,
        environments,
        root_auth: ImportedAuth::None,
        root_headers: Vec::new(),
        items,
        warnings,
    })
}

/// `{{ _['name'] }}` / `{{ _["name"] }}` → `{{ name }}` (Insomnia's syntax for
/// names with dashes). Other spans are left untouched.
fn normalize_brackets(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        out.push_str(&rest[..start]);
        let inner = after[..end].trim();
        let name = inner
            .strip_prefix("_[")
            .and_then(|s| s.strip_suffix(']'))
            .map(|s| s.trim_matches(|c| c == '\'' || c == '"' || c == '\\'));
        match name {
            Some(n) if !n.is_empty() => {
                out.push_str("{{ ");
                out.push_str(n);
                out.push_str(" }}");
            }
            _ => out.push_str(&rest[start..start + 2 + end + 2]),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Children of `parent_id`, ordered by `metaSortKey`.
/// `seen` guards against a `parentId` cycle in a crafted export (mirrors
/// `util::RefResolver`) — a repeated group id stops the walk.
fn build_children<'a>(
    resources: &'a [Value],
    parent_id: &str,
    notes: &mut Notes,
    seen: &mut HashSet<&'a str>,
) -> Vec<ImportedItem> {
    let mut kids: Vec<&Value> = resources
        .iter()
        .filter(|r| parent_of(r) == parent_id && matches!(type_of(r), "request_group" | "request"))
        .collect();
    kids.sort_by(|a, b| {
        sort_key(a)
            .partial_cmp(&sort_key(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut out = Vec::new();
    for r in kids {
        if type_of(r) == "request_group" {
            // A parentId cycle would revisit this group id — stop instead of recursing forever.
            if !seen.insert(id_of(r)) {
                continue;
            }
            out.push(ImportedItem::Folder(ImportedFolder {
                name: name_of(r),
                description: r
                    .get("description")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
                auth: r.get("authentication").map(to_auth).unwrap_or_default(),
                headers: headers_of(r),
                variables: notes.env(r.get("environment")),
                items: build_children(resources, id_of(r), notes, seen),
            }));
        } else {
            out.push(ImportedItem::Request(build_request(r)));
        }
    }
    out
}

pub(crate) fn build_request(r: &Value) -> ImportedRequest {
    let url = s(r, "url");
    let mut method = s(r, "method").to_uppercase();
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
    req.query = query_of(r);
    if req.query.is_empty() {
        req.query = query_from_url(&url);
    }
    req.path_params = name_value_list(r.get("pathParameters"));
    req.headers = headers_of(r);
    req.body = parse_body(r.get("body"));
    req.auth = r.get("authentication").map(to_auth).unwrap_or_default();
    req
}

fn parse_body(body: Option<&Value>) -> Option<ImportedBody> {
    let body = body?;
    let mime = body.get("mimeType").and_then(Value::as_str).unwrap_or("");
    let file = s(body, "fileName");

    if mime.contains("graphql") {
        // `text` is a JSON blob: { "query": "...", "variables": {...} }.
        let parsed: Value = serde_json::from_str(&s(body, "text")).unwrap_or(Value::Null);
        return Some(ImportedBody::GraphQl {
            query: parsed
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            variables: parsed.get("variables").filter(|v| !v.is_null()).map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| v.to_string())
            }),
        });
    }
    if mime.contains("x-www-form-urlencoded") {
        return Some(ImportedBody::FormUrlEncoded(params_to_fields(
            body.get("params"),
        )));
    }
    if mime.contains("multipart/form-data") {
        return Some(ImportedBody::Multipart(params_to_fields(
            body.get("params"),
        )));
    }
    if !file.is_empty() {
        return Some(ImportedBody::Binary(file));
    }
    let text = s(body, "text");
    if text.is_empty() && mime.is_empty() {
        return None;
    }
    Some(ImportedBody::Raw {
        hint: raw_kind(mime),
        text,
    })
}

/// Split environments: the base env (parent is the workspace) → Global variables;
/// every other `environment` resource → a named Voleeo environment.
fn collect_environments(
    resources: &[Value],
    root_id: &str,
    notes: &mut Notes,
) -> (Vec<ImportedVariable>, Vec<ImportedEnvironment>) {
    let mut base = Vec::new();
    let mut named = Vec::new();
    for e in resources.iter().filter(|r| type_of(r) == "environment") {
        let vars = notes.env_of(e);
        if parent_of(e) == root_id {
            base = vars;
        } else {
            named.push(ImportedEnvironment {
                name: name_of(e),
                variables: vars,
            });
        }
    }
    (base, named)
}

pub(crate) fn headers_of(v: &Value) -> Vec<ImportedParam> {
    name_value_list(v.get("headers"))
}

fn query_of(v: &Value) -> Vec<ImportedParam> {
    name_value_list(v.get("parameters"))
}

/// Insomnia's `[{name, value, disabled}]` → params (skips unnamed entries).
fn name_value_list(v: Option<&Value>) -> Vec<ImportedParam> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let name = p.get("name").and_then(Value::as_str)?;
                    if name.is_empty() {
                        return None;
                    }
                    Some(ImportedParam {
                        name: name.to_string(),
                        value: s(p, "value"),
                        enabled: !disabled(p),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn params_to_fields(v: Option<&Value>) -> Vec<ImportedField> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    let name = p.get("name").and_then(Value::as_str)?;
                    let file = s(p, "fileName");
                    let is_file =
                        p.get("type").and_then(Value::as_str) == Some("file") || !file.is_empty();
                    Some(ImportedField {
                        name: name.to_string(),
                        value: if is_file { file } else { s(p, "value") },
                        is_file,
                        enabled: !disabled(p),
                        content_type: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn query_from_url(url: &str) -> Vec<ImportedParam> {
    url.split_once('?')
        .map(|(_, qs)| {
            qs.split('&')
                .filter(|s| !s.is_empty())
                .filter_map(|pair| {
                    let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
                    if k.is_empty() {
                        return None;
                    }
                    Some(ImportedParam {
                        name: k.to_string(),
                        value: v.to_string(),
                        enabled: true,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn raw_kind(mime: &str) -> RawKind {
    if mime.contains("json") {
        RawKind::Json
    } else if mime.contains("xml") {
        RawKind::Xml
    } else if mime.contains("html") {
        RawKind::Html
    } else {
        RawKind::Text
    }
}

/// True if any `{{ … }}` span carries a Nunjucks `|` filter (which we can't run).
/// Scans the spans precisely so `|` in plain JSON values doesn't false-positive.
fn has_template_filter(content: &str) -> bool {
    let mut rest = content;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        if after[..end].contains('|') {
            return true;
        }
        rest = &after[end + 2..];
    }
    false
}

fn type_of(v: &Value) -> &str {
    v.get("_type").and_then(Value::as_str).unwrap_or("")
}
fn id_of(v: &Value) -> &str {
    v.get("_id").and_then(Value::as_str).unwrap_or("")
}
fn parent_of(v: &Value) -> &str {
    v.get("parentId").and_then(Value::as_str).unwrap_or("")
}
fn sort_key(v: &Value) -> f64 {
    v.get("metaSortKey").and_then(Value::as_f64).unwrap_or(0.0)
}
pub(crate) fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}
