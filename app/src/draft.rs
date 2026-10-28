//! Editor state -> model mapping: what the request would look like if saved.

use yuki_core::{
    new_id, BodyField, BodyKind, DnsOverride, HttpRequest, RequestBody, RequestParameter, Variation,
};

/// Table cells are single-line inputs: line breaks show as spaces.
pub fn one_line(s: &str) -> String {
    s.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

/// An untouched cell keeps the stored value even if it had line breaks —
/// rewriting it on the next save would corrupt the user's data.
pub fn cell_value(shown: &str, stored: &str) -> String {
    if shown == one_line(stored) {
        stored.to_string()
    } else {
        shown.to_string()
    }
}

/// What the body pane holds. Each kind keeps its own input, so switching
/// kinds never drops what was typed in another.
#[derive(Debug, Clone, PartialEq)]
pub struct BodyDraft {
    pub kind: BodyKind,
    pub text: String,
    pub graphql_vars: String,
    pub fields: Vec<BodyField>,
    pub file: String,
}

impl BodyDraft {
    /// No stored body opens as JSON: typing is enough to create one.
    pub fn from(body: Option<&RequestBody>) -> Self {
        let b = body.cloned().unwrap_or(RequestBody {
            kind: BodyKind::Json,
            ..Default::default()
        });
        Self {
            kind: b.kind,
            text: b.text,
            graphql_vars: b.graphql_variables.unwrap_or_default(),
            fields: b.fields.unwrap_or_default(),
            file: b.file_path.unwrap_or_default(),
        }
    }
}

pub fn apply_body(prev: Option<&RequestBody>, d: &BodyDraft) -> Option<RequestBody> {
    let base = prev.cloned().unwrap_or_default();
    // Unset optionals stay unset while empty — no phantom diff on save.
    let keep = |s: &str, was: bool| (was || !s.is_empty()).then(|| s.to_string());
    let body = RequestBody {
        kind: d.kind.clone(),
        text: d.text.clone(),
        fields: (base.fields.is_some() || !d.fields.is_empty()).then(|| d.fields.clone()),
        file_path: keep(&d.file, base.file_path.is_some()),
        graphql_variables: keep(&d.graphql_vars, base.graphql_variables.is_some()),
        ..base
    };
    let empty = RequestBody {
        kind: body.kind.clone(),
        ..Default::default()
    };
    (prev.is_some() || body != empty).then_some(body)
}

/// The request as the editor would save it.
pub fn build(
    saved: &HttpRequest,
    method: &str,
    url: &str,
    headers: Vec<RequestParameter>,
    body: &BodyDraft,
) -> HttpRequest {
    HttpRequest {
        method: method.to_string(),
        url: url.to_string(),
        headers,
        body: apply_body(saved.body.as_ref(), body),
        ..saved.clone()
    }
}

/// `edited` (what the editor shows) as it goes to disk over `saved`: into the
/// active variation (all five fields — each variation is an independent copy),
/// else into the base. Method and settings are base-only. Identity (name,
/// folder, order…) always comes from `saved`.
pub fn fold(saved: &HttpRequest, edited: HttpRequest) -> HttpRequest {
    let mut out = HttpRequest {
        method: edited.method.clone(),
        settings: edited.settings.clone(),
        ..saved.clone()
    };
    match out.active_mut() {
        Some(v) => *v = Variation::copy_of(&edited, v.id.clone(), v.name.clone()),
        None => {
            out.url = edited.url;
            out.parameters = edited.parameters;
            out.headers = edited.headers;
            out.body = edited.body;
            out.auth = edited.auth;
        }
    }
    out
}

/// The stored params with the URL's `:name` values applied. Params whose
/// `:name` left the URL are dropped (they'd turn into a query); others
/// (imported query params) ride along. Order and ids are kept and empty new
/// names aren't stored, so an untouched request rebuilds exactly.
pub fn merge_path_params(
    saved_url: &str,
    saved: &[RequestParameter],
    path: &[(String, String)],
) -> Vec<RequestParameter> {
    let was_path = yuki_mcp::resolve::path_params(saved_url);
    let mut out: Vec<RequestParameter> = saved
        .iter()
        .filter_map(|p| match path.iter().find(|(n, _)| *n == p.name) {
            Some((_, v)) => Some(RequestParameter {
                value: v.clone(),
                ..p.clone()
            }),
            None => (!was_path.contains(&p.name)).then(|| p.clone()),
        })
        .collect();
    for (name, value) in path {
        if !value.is_empty() && !saved.iter().any(|p| p.name == *name) {
            out.push(RequestParameter {
                id: new_id(),
                name: name.clone(),
                value: value.clone(),
                enabled: true,
            });
        }
    }
    out
}

/// DNS overrides as `/etc/hosts` lines (`127.0.0.1 api.local`); `#` = disabled.
pub fn hosts_text(rows: &[DnsOverride]) -> String {
    let line = |r: &DnsOverride| {
        let l = format!("{} {}", r.address, r.hostname);
        let l = l.trim();
        if r.enabled {
            l.to_string()
        } else {
            format!("# {l}")
        }
    };
    rows.iter().map(line).collect::<Vec<_>>().join("\n")
}

/// Lines back to overrides. Ids are kept by position: fresh ids would be a
/// phantom diff in the user's git.
pub fn hosts_rows(text: &str, prev: &[DnsOverride]) -> Vec<DnsOverride> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let (enabled, l) = match l.strip_prefix('#') {
                Some(rest) => (false, rest.trim()),
                None => (true, l),
            };
            let mut parts = l.split_whitespace();
            let address = parts.next()?.to_string();
            let hostname = parts.next().unwrap_or_default().to_string();
            Some((enabled, address, hostname))
        })
        .enumerate()
        .map(|(i, (enabled, address, hostname))| DnsOverride {
            id: prev.get(i).map_or_else(new_id, |p| p.id.clone()),
            enabled,
            hostname,
            address,
        })
        .collect()
}

/// Re-indents JSON (2 spaces) token by token instead of parsing it, so key
/// order, number spelling and unquoted `{{ VAR }}` survive; only whitespace
/// outside strings changes. `None` when brackets or strings don't balance.
pub fn pretty_json(text: &str) -> Option<String> {
    fn newline(out: &mut String, depth: usize) {
        out.push('\n');
        out.push_str(&"  ".repeat(depth));
    }
    let mut out = String::with_capacity(text.len() * 2);
    let mut open: Vec<char> = vec![];
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push(c);
                loop {
                    let c = chars.next()?;
                    out.push(c);
                    match c {
                        '\\' => out.push(chars.next()?),
                        '"' => break,
                        _ => {}
                    }
                }
            }
            // `{{` can't open an object (keys are strings): it's a template.
            '{' if chars.peek() == Some(&'{') => {
                out.push(c);
                loop {
                    let c = chars.next()?;
                    out.push(c);
                    if c == '}' && chars.next_if_eq(&'}').is_some() {
                        out.push('}');
                        break;
                    }
                }
            }
            '{' | '[' => {
                let close = if c == '{' { '}' } else { ']' };
                out.push(c);
                while chars.next_if(|c| c.is_whitespace()).is_some() {}
                if chars.next_if_eq(&close).is_some() {
                    out.push(close);
                } else {
                    open.push(close);
                    newline(&mut out, open.len());
                }
            }
            '}' | ']' => {
                if open.pop()? != c {
                    return None;
                }
                newline(&mut out, open.len());
                out.push(c);
            }
            ',' => {
                out.push(c);
                newline(&mut out, open.len());
            }
            ':' => out.push_str(": "),
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    open.is_empty().then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_json_keeps_content_and_templates() {
        let src = r#"{"b":1.50,"a":[ ],"t":{{ ID }},"s":"x, {y}: \"z\"","o":{"n":[1,{}]}}"#;
        let want = "{\n  \"b\": 1.50,\n  \"a\": [],\n  \"t\": {{ ID }},\n  \"s\": \"x, {y}: \\\"z\\\"\",\n  \"o\": {\n    \"n\": [\n      1,\n      {}\n    ]\n  }\n}";
        assert_eq!(pretty_json(src).as_deref(), Some(want));
        // Already formatted: no-op.
        assert_eq!(pretty_json(want).as_deref(), Some(want));
        for bad in [r#"{"a":1"#, r#"{"a":"1}"#, r#"{"a":[1}"#, "]"] {
            assert_eq!(pretty_json(bad), None, "{bad}");
        }
    }

    #[test]
    fn variations_are_independent_copies() {
        let bearer = |t: &str| yuki_core::AuthConfig::Bearer {
            token: t.into(),
            token_encrypted: false,
            enabled: true,
        };
        let base: HttpRequest = serde_json::from_value(serde_json::json!({
            "id": "r", "type": "api", "model": "http_request", "workspaceId": "w",
            "folderId": null, "method": "GET", "name": "n", "url": "https://base",
            "createdAt": "", "updatedAt": ""
        }))
        .unwrap();
        // An older file: the variation holds only auth.
        let mut saved = HttpRequest {
            auth: bearer("base"),
            variations: vec![yuki_core::Variation {
                id: "v".into(),
                name: "admin".into(),
                auth: Some(bearer("admin")),
                ..Default::default()
            }],
            active_variation: Some("v".into()),
            ..base
        };
        saved.detach_variations();
        assert_eq!(saved.variations[0].url.as_deref(), Some("https://base"));
        let shown = saved.shown();
        // Untouched form: same on disk (no phantom save).
        assert_eq!(fold(&saved, shown.clone()), saved);

        // URL edited while `admin` is active: only the variation changes.
        let out = fold(
            &saved,
            HttpRequest {
                url: "https://admin".into(),
                ..shown.clone()
            },
        );
        assert_eq!(
            (out.url.as_str(), &out.auth),
            ("https://base", &bearer("base"))
        );
        assert_eq!(out.variations[0].url.as_deref(), Some("https://admin"));
        assert_eq!(out.variations[0].auth, Some(bearer("admin")));

        // Settings edited while `admin` is active: they're the base's.
        let mut timed = shown.clone();
        timed.settings.timeout_ms = Some(500);
        let out = fold(&saved, timed);
        assert_eq!(out.settings.timeout_ms, Some(500));
        assert_eq!(out.shown().settings.timeout_ms, Some(500));

        // Base active: edits go to the base and never reach the variation;
        // identity always from `saved`.
        let on_base = HttpRequest {
            active_variation: None,
            name: "renamed on disk".into(),
            ..saved.clone()
        };
        let out = fold(
            &on_base,
            HttpRequest {
                url: "https://new".into(),
                ..on_base.shown()
            },
        );
        assert_eq!(
            (out.url.as_str(), out.name.as_str()),
            ("https://new", "renamed on disk")
        );
        assert_eq!(out.variations, saved.variations);
    }

    #[test]
    fn untouched_multiline_cell_keeps_stored_value() {
        let stored = "l1\r\nl2\nl3";
        assert_eq!(one_line(stored), "l1 l2 l3");
        assert_eq!(cell_value("l1 l2 l3", stored), stored);
        assert_eq!(cell_value("l1 l2", stored), "l1 l2");
    }

    #[test]
    fn path_params_merge_keeps_query_and_drops_stale() {
        let p = |name: &str, value: &str| RequestParameter {
            id: name.into(),
            name: name.into(),
            value: value.into(),
            enabled: true,
        };
        let saved = vec![p("id", "7"), p("limit", "10"), p("tab", "x")];
        let url = "/users/:id/:tab";
        let path = |v: &[(&str, &str)]| -> Vec<(String, String)> {
            v.iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect()
        };
        // Untouched: identical (no phantom save).
        let same = path(&[("id", "7"), ("tab", "x")]);
        assert_eq!(merge_path_params(url, &saved, &same), saved);
        // `:tab` removed, `:org` added: id edited, limit (query) kept.
        let got = merge_path_params(url, &saved, &path(&[("id", "8"), ("org", "acme")]));
        let names: Vec<_> = got
            .iter()
            .map(|p| (p.name.as_str(), p.value.as_str()))
            .collect();
        assert_eq!(names, [("id", "8"), ("limit", "10"), ("org", "acme")]);
        // A new empty `:name` isn't stored.
        assert!(merge_path_params("/u/:id", &[], &path(&[("id", "")])).is_empty());
    }

    #[test]
    fn stored_bodies_reload_without_diff() {
        let bodies = [
            RequestBody {
                kind: BodyKind::FormUrlEncoded,
                fields: Some(vec![]),
                ..Default::default()
            },
            RequestBody {
                kind: BodyKind::Binary,
                file_path: Some("/tmp/x.bin".into()),
                content_type: Some("application/pdf".into()),
                ..Default::default()
            },
            RequestBody {
                kind: BodyKind::Graphql,
                text: "{ me }".into(),
                graphql_variables: Some("{}".into()),
                ..Default::default()
            },
            RequestBody {
                kind: BodyKind::None,
                ..Default::default()
            },
        ];
        for b in bodies {
            assert_eq!(apply_body(Some(&b), &BodyDraft::from(Some(&b))), Some(b));
        }
        assert_eq!(apply_body(None, &BodyDraft::from(None)), None);
    }

    #[test]
    fn switching_kind_keeps_other_inputs() {
        let form = RequestBody {
            kind: BodyKind::FormUrlEncoded,
            text: "{}".into(),
            fields: Some(vec![]),
            ..Default::default()
        };
        let mut d = BodyDraft::from(Some(&form));
        d.kind = BodyKind::Json;
        let json = apply_body(Some(&form), &d).unwrap();
        assert_eq!((json.kind, json.text), (BodyKind::Json, "{}".into()));
        assert_eq!(json.fields, form.fields);
        let mut typed = BodyDraft::from(None);
        typed.text = "{}".into();
        assert_eq!(apply_body(None, &typed).unwrap().kind, BodyKind::Json);
    }

    #[test]
    fn hosts_text_roundtrips_and_keeps_ids() {
        let row = |id: &str, enabled, address: &str, hostname: &str| DnsOverride {
            id: id.into(),
            enabled,
            hostname: hostname.into(),
            address: address.into(),
        };
        let rows = vec![
            row("a", true, "127.0.0.1", "api.local"),
            row("b", false, "::1", "v6.local"),
        ];
        let text = hosts_text(&rows);
        assert_eq!(text, "127.0.0.1 api.local\n# ::1 v6.local");
        assert_eq!(hosts_rows(&text, &rows), rows);
        // Blank and bare `#` lines vanish; a new line gets a fresh id.
        let edited = hosts_rows(
            "\n#\n10.0.0.1 a.local\n  #10.0.0.2   b.local  \n1.1.1.1 c",
            &rows,
        );
        assert_eq!(edited.len(), 3);
        assert_eq!((edited[0].id.as_str(), edited[0].enabled), ("a", true));
        assert_eq!((edited[1].id.as_str(), edited[1].enabled), ("b", false));
        assert_eq!(edited[1].hostname, "b.local");
        assert!(!edited[2].id.is_empty() && edited[2].id != "a" && edited[2].id != "b");
    }
}
