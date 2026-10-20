//! `{{ $response.body(requestId="…", selector="$.path") }}`: a field of another
//! request's latest stored response — a pointer: `send_http` re-sends every
//! source the request reaches (`sources`) before resolving it, so the value is
//! fresh. `resolve_str` has no stores, so callers prefill: each call found in
//! their texts is evaluated once and stored in the vars map under its
//! expression, where `resolve_str` looks it up. A call with no stored response
//! or no match stays out — its token is sent verbatim.

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use yuki_core::HttpRequest;
use yuki_storage::ResponseStore;

use super::{funcs, vars::is_identifier};

/// Evaluate every `$response.body(…)` in the var values and, when given, in
/// any string field of `req`.
pub fn prefill_responses(
    vars: &mut HashMap<String, String>,
    req: Option<&HttpRequest>,
    ws_id: &str,
    responses: &ResponseStore,
) {
    let mut texts: Vec<String> = vars.values().cloned().collect();
    if let Some(v) = req.and_then(|r| serde_json::to_value(r).ok()) {
        strings(&v, &mut texts);
    }
    let mut exprs: Vec<String> = Vec::new();
    for text in &texts {
        for inner in tokens(text) {
            if args(inner).is_some() && !exprs.iter().any(|e| e == inner) {
                exprs.push(inner.to_string());
            }
        }
    }
    let found: Vec<(String, String)> = exprs
        .into_iter()
        .filter_map(|e| Some((e.clone(), body(&e, ws_id, responses)?)))
        .collect();
    vars.extend(found);
}

/// The requests whose responses `req` reads through `$response.body` — in its
/// own fields or in the variables it uses, transitively. In discovery order,
/// no duplicates.
pub fn sources(vars: &HashMap<String, String>, req: &HttpRequest) -> Vec<String> {
    let mut texts = Vec::new();
    if let Ok(v) = serde_json::to_value(req) {
        strings(&v, &mut texts);
    }
    sources_from(vars, texts)
}

fn sources_from(vars: &HashMap<String, String>, mut texts: Vec<String>) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<String> = Vec::new();
    while let Some(text) = texts.pop() {
        for inner in tokens(&text) {
            if is_identifier(inner) {
                if seen.insert(inner.to_string()) {
                    texts.extend(vars.get(inner).cloned());
                }
            } else if let Some(id) = source_of(inner).filter(|id| !out.contains(id)) {
                out.push(id);
            }
        }
    }
    out
}

/// The arguments of a response call: `$response.body(…)`, or Voleeo's
/// sigil-less `response.body(…)` that imported workspaces carry. Either way it
/// is a pointer (sent first); Voleeo's `strategy`/`ttl` args are ignored —
/// `force` behaviour, which is what keeps the value fresh.
pub fn args(expr: &str) -> Option<Vec<(&str, String)>> {
    let (name, args) = funcs::call(expr.strip_prefix('$').unwrap_or(expr))?;
    (name == "response.body").then_some(args)
}

fn source_of(expr: &str) -> Option<String> {
    args(expr)?
        .into_iter()
        .find(|(k, _)| *k == "requestId")
        .map(|(_, v)| v)
}

/// The trimmed inside of every `{{ … }}` in `text`.
fn tokens(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("{{") {
        rest = &rest[open + 2..];
        let Some(close) = rest.find("}}") else { break };
        out.push(rest[..close].trim());
        rest = &rest[close + 2..];
    }
    out
}

fn strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) if s.contains("{{") => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|v| strings(v, out)),
        Value::Object(o) => o.values().for_each(|v| strings(v, out)),
        _ => {}
    }
}

fn body(expr: &str, ws_id: &str, responses: &ResponseStore) -> Option<String> {
    let args = args(expr)?;
    let arg = |k: &str| args.iter().find(|(n, _)| *n == k).map(|(_, v)| v.as_str());
    let stored = responses.latest(ws_id, arg("requestId")?).ok()??;
    let body = responses.read_full_body(ws_id, &stored).ok()?;
    select(&body, arg("selector").unwrap_or(""))
}

/// Empty selector → the whole body; otherwise JSONPath. A single string match
/// is its text, anything else its JSON; no match → `None`.
pub fn select(body: &str, selector: &str) -> Option<String> {
    let s = selector.trim();
    if s.is_empty() {
        return Some(body.to_string());
    }
    let json: Value = serde_json::from_str(body).ok()?;
    let path = serde_json_path::JsonPath::parse(s).ok()?;
    match path.query(&json).all().as_slice() {
        [] => None,
        [Value::String(s)] => Some(s.clone()),
        [one] => Some(one.to_string()),
        many => serde_json::to_string(many).ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_by_json_path() {
        let body = r#"{"data":{"token":"abc","n":3,"tags":["a","b"]}}"#;
        assert_eq!(select(body, "$.data.token").as_deref(), Some("abc"));
        assert_eq!(select(body, "$.data.n").as_deref(), Some("3"));
        assert_eq!(
            select(body, "$.data.tags[*]").as_deref(),
            Some(r#"["a","b"]"#)
        );
        assert_eq!(select(body, "").as_deref(), Some(body));
        assert_eq!(select(body, "$.missing"), None);
        assert_eq!(select("not json", "$.a"), None);
    }

    #[test]
    fn sources_follow_used_vars_only() {
        let call =
            |id: &str| format!(r#"{{{{ $response.body(requestId="{id}", selector="$.t") }}}}"#);
        let vars: HashMap<String, String> = [
            ("T", "{{ A }}".to_string()),
            ("A", call("login")),
            ("UNUSED", call("other")),
            ("LOOP", "{{ LOOP }}".to_string()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let texts = vec!["Bearer {{ T }} {{ LOOP }}".to_string(), call("login")];
        assert_eq!(sources_from(&vars, texts), ["login"]);
        // Voleeo's form (imported data): no `$`, extra args.
        let voleeo = r#"{{ response.body(requestId="auth", strategy="force", selector="$.t") }}"#;
        assert_eq!(sources_from(&vars, vec![voleeo.into()]), ["auth"]);
        assert!(args("responsebody()").is_none() && args("$fake.email()").is_none());
    }
}
