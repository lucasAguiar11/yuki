//! Postman Collection v2.0 / v2.1 (and environment/globals files) → IR. One
//! parser for both collection versions (they differ only in `info.schema` and
//! the auth field shape, see `auth`). `item[]` nests to arbitrary depth (a node
//! with `item` is a folder, one with `request` is a request); `url` may be a
//! string or an object with `query`/`variable` arrays; `body` has five modes.
//! Postman `{{var}}` tokens already match Voleeo's syntax, so variable keys pass
//! through verbatim — never sanitized (that would break the references). A
//! missing `auth` inherits the enclosing scope. `event[]` scripts aren't
//! importable → counted into one warning.

mod auth;

use crate::ir::*;
use crate::util::{disabled, name_of, parse_value, str_at_or, strip_query};
use crate::ImportError;
use auth::to_auth;
use serde_json::Value;

/// Warnings + the running script count, threaded through the recursive walk.
#[derive(Default)]
struct Acc {
    warnings: Vec<String>,
    scripts: u32,
}

pub fn parse_postman(content: &str) -> Result<ImportedCollection, ImportError> {
    let doc = parse_value(content)?;
    if doc.get("item").is_none() {
        return match doc.get("values") {
            Some(values) if values.is_array() => Ok(environment(&doc, values)),
            _ => Err(ImportError::Parse(
                "missing `item` array (not a Postman collection)".into(),
            )),
        };
    }
    let version = if str_at_or(&doc, "/info/schema").contains("v2.0") {
        "2.0"
    } else {
        "2.1"
    };

    let mut acc = Acc::default();
    count_scripts(&doc, &mut acc);
    let items = match doc.get("item") {
        Some(Value::Array(arr)) => build_items(arr, &mut acc),
        _ => Vec::new(),
    };
    if acc.scripts > 0 {
        let (plural, verb) = if acc.scripts == 1 {
            ("", "was")
        } else {
            ("s", "were")
        };
        acc.warnings.push(format!(
            "{n} pre-request/test script{plural} {verb} detected and skipped — Yuki doesn't run scripts.",
            n = acc.scripts
        ));
    }
    let dynamic = content.matches("{{$").count();
    if dynamic > 0 {
        acc.warnings.push(format!(
            "{dynamic} Postman dynamic variable reference(s) ({{{{$guid}}}}, {{{{$timestamp}}}}, {{{{$random…}}}}) kept as-is — Yuki doesn't resolve them."
        ));
    }

    Ok(ImportedCollection {
        name: doc
            .pointer("/info/name")
            .and_then(Value::as_str)
            .unwrap_or("Imported API")
            .to_string(),
        version: Some(version.to_string()),
        root_headers: Vec::new(),
        variables: variables(doc.get("variable")),
        environments: Vec::new(),
        // Absent root auth = Inherit, which resolves to none at the root.
        root_auth: to_auth(doc.get("auth")),
        items,
        warnings: acc.warnings,
    })
}

/// Environment / globals export: `{name, values[], _postman_variable_scope?}`.
/// Globals become the collection variables; an environment becomes one named env.
fn environment(doc: &Value, values: &Value) -> ImportedCollection {
    let name = doc
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Imported environment")
        .to_string();
    let vars = variables(Some(values));
    let mut col = ImportedCollection {
        name: name.clone(),
        ..Default::default()
    };
    if str_at_or(doc, "/_postman_variable_scope") == "globals" {
        col.variables = vars;
    } else {
        col.environments.push(ImportedEnvironment {
            name,
            variables: vars,
        });
    }
    col
}

fn build_items(items: &[Value], acc: &mut Acc) -> Vec<ImportedItem> {
    let mut out = Vec::new();
    for it in items {
        count_scripts(it, acc);
        if let Some(Value::Array(children)) = it.get("item") {
            out.push(ImportedItem::Folder(ImportedFolder {
                name: name_of(it),
                description: it
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                auth: to_auth(it.get("auth")),
                headers: Vec::new(),
                variables: variables(it.get("variable")),
                items: build_items(children, acc),
            }));
        } else if let Some(req) = it.get("request") {
            out.push(ImportedItem::Request(build_request(name_of(it), req)));
        }
    }
    out
}

fn build_request(name: String, req: &Value) -> ImportedRequest {
    // v2.0 shorthand: `request` is just a URL string.
    if let Some(raw) = req.as_str() {
        let mut r = blank_request(name, "GET");
        set_url_string(raw, &mut r);
        return r;
    }

    let method = req
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("GET")
        .to_uppercase();
    let mut r = blank_request(name, &method);

    if let Some(url) = req.get("url") {
        parse_url(url, &mut r);
    }
    r.headers = params(req.get("header"));
    r.body = req.get("body").and_then(parse_body);
    r.auth = to_auth(req.get("auth"));
    r
}

fn parse_url(url: &Value, r: &mut ImportedRequest) {
    if let Some(raw) = url.as_str() {
        set_url_string(raw, r);
        return;
    }
    // `raw` is usually present; fall back to the host/path arrays when it isn't.
    let raw = match url.get("raw").and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => reconstruct_raw(url),
    };
    r.query = params(url.get("query"));
    if r.query.is_empty() {
        // No structured `query[]` (hand-written or older exports): the query
        // lives only in `raw`.
        set_url_string(&raw, r);
    } else {
        r.url = strip_query(&raw).to_string();
        r.path = raw;
    }
    r.path_params = params(url.get("variable"));
}

/// String URL: store it query-stripped and split any `?a=b&c=d` into params.
fn set_url_string(raw: &str, r: &mut ImportedRequest) {
    r.url = strip_query(raw).to_string();
    r.path = raw.to_string();
    if let Some((_, qs)) = raw.split_once('?') {
        for pair in qs.split('&').filter(|s| !s.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            if k.is_empty() {
                continue;
            }
            r.query.push(ImportedParam {
                name: k.to_string(),
                value: v.to_string(),
                enabled: true,
            });
        }
    }
}

/// `[{key, value, disabled}]` (or env `enabled: false`) → params, skipping empty keys.
fn params(arr: Option<&Value>) -> Vec<ImportedParam> {
    arr.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let key = p
                .get("key")
                .and_then(Value::as_str)
                .filter(|k| !k.is_empty())?;
            Some(ImportedParam {
                name: key.to_string(),
                value: p.get("value").map(scalar).unwrap_or_default(),
                enabled: !disabled(p) && p.get("enabled").and_then(Value::as_bool) != Some(false),
            })
        })
        .collect()
}

fn parse_body(body: &Value) -> Option<ImportedBody> {
    match body.get("mode").and_then(Value::as_str)? {
        "raw" => Some(ImportedBody::Raw {
            hint: raw_kind(str_at_or(body, "/options/raw/language")),
            text: body
                .get("raw")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        }),
        "urlencoded" => Some(ImportedBody::FormUrlEncoded(form_fields(
            body.get("urlencoded"),
        ))),
        "formdata" => Some(ImportedBody::Multipart(form_fields(body.get("formdata")))),
        "graphql" => Some(ImportedBody::GraphQl {
            query: str_at_or(body, "/graphql/query").to_string(),
            variables: body
                .pointer("/graphql/variables")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        }),
        "file" => Some(ImportedBody::Binary(file_src(body.pointer("/file/src")))),
        _ => None,
    }
}

fn form_fields(arr: Option<&Value>) -> Vec<ImportedField> {
    arr.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|f| {
                    let name = f.get("key").and_then(Value::as_str)?;
                    let is_file = f.get("type").and_then(Value::as_str) == Some("file");
                    Some(ImportedField {
                        name: name.to_string(),
                        value: if is_file {
                            file_src(f.get("src"))
                        } else {
                            f.get("value").map(scalar).unwrap_or_default()
                        },
                        is_file,
                        content_type: None,
                        enabled: !disabled(f),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// File `src`: a path string, or an array of paths (first wins).
fn file_src(src: Option<&Value>) -> String {
    match src {
        Some(Value::Array(a)) => a.first().map(scalar).unwrap_or_default(),
        Some(v) => scalar(v),
        None => String::new(),
    }
}

/// A JSON value as text: strings verbatim, `null` empty, numbers/bools printed.
fn scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Collection/folder `variable[]` (`disabled: true`) and environment `values[]`
/// (`enabled: false`).
fn variables(v: Option<&Value>) -> Vec<ImportedVariable> {
    params(v)
        .into_iter()
        .map(|p| ImportedVariable {
            key: p.name,
            value: p.value,
            enabled: p.enabled,
        })
        .collect()
}

fn count_scripts(node: &Value, acc: &mut Acc) {
    if let Some(Value::Array(events)) = node.get("event") {
        for e in events {
            if e.pointer("/script/exec")
                .and_then(Value::as_array)
                .is_some_and(|x| {
                    x.iter()
                        .any(|l| l.as_str().is_some_and(|s| !s.trim().is_empty()))
                })
            {
                acc.scripts += 1;
            }
        }
    }
}

fn blank_request(name: String, method: &str) -> ImportedRequest {
    ImportedRequest {
        name,
        method: method.to_string(),
        ..Default::default()
    }
}

fn raw_kind(lang: &str) -> RawKind {
    match lang {
        "json" => RawKind::Json,
        "xml" => RawKind::Xml,
        "html" => RawKind::Html,
        _ => RawKind::Text,
    }
}

/// Rebuild a URL from the `host`/`path` arrays when `url.raw` is missing.
fn reconstruct_raw(url: &Value) -> String {
    let host = join_arr(url.get("host"), ".");
    let path = join_arr(url.get("path"), "/");
    match (host.is_empty(), path.is_empty()) {
        (true, true) => String::new(),
        (false, true) => host,
        (true, false) => format!("/{path}"),
        (false, false) => format!("{host}/{path}"),
    }
}

fn join_arr(v: Option<&Value>, sep: &str) -> String {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(sep)
        })
        .unwrap_or_default()
}
