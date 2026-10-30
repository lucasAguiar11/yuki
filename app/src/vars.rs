//! `{{ … }}` template variables: where they sit in text, what they resolve to,
//! and which layer defines them.

use std::{
    collections::{BTreeMap, HashMap},
    ops::Range,
};

use gpui_kit::component::input::{InlineToken, InputContent};
use yuki_mcp::resolve::{funcs, is_identifier, response_args};

/// The layer a variable comes from, strongest first.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Folder { name: String, id: String },
    Env(String),
    Global,
    System,
}

impl Source {
    pub fn label(&self) -> String {
        match self {
            Source::Folder { name, .. } => t!("folder · %{name}", name = name).into_owned(),
            Source::Env(name) => name.clone(),
            Source::Global => "global".into(),
            Source::System => t!("system").into_owned(),
        }
    }
}

/// Where ⌘-click on a chip goes.
#[derive(Clone, PartialEq, Debug)]
pub enum Origin {
    /// `$response.body(…)`: the request whose response it reads.
    Request(String),
    /// A `:name` path param: its row in the params tab.
    PathParam(String),
    Folder {
        id: String,
        var: String,
    },
    /// The active env (global when none) or the global one; a missing `var`
    /// is started there.
    Env {
        global: bool,
        var: String,
    },
}

pub fn origin(expr: &str, vars: &Vars) -> Option<Origin> {
    if let Some(name) = expr.strip_prefix(':') {
        return Some(Origin::PathParam(name.to_string()));
    }
    if response_args(expr).is_some() {
        return response_ref(expr).map(|(id, _)| Origin::Request(id));
    }
    // Imported names like `api-gateway` aren't identifiers but still resolve.
    if !is_identifier(expr) && !vars.contains_key(expr) {
        return None;
    }
    let var = expr.to_string();
    match vars.get(expr).map(|v| &v.source) {
        Some(Source::Folder { id, .. }) => Some(Origin::Folder {
            id: id.clone(),
            var,
        }),
        Some(Source::Env(_)) | None => Some(Origin::Env { global: false, var }),
        Some(Source::Global) => Some(Origin::Env { global: true, var }),
        Some(Source::System) => None,
    }
}

#[derive(Clone, Debug)]
pub struct Var {
    /// As stored (may itself contain `{{ … }}`).
    pub raw: String,
    /// Fully resolved, what the request would send.
    pub value: String,
    pub source: Source,
    pub secret: bool,
}

pub type Vars = BTreeMap<String, Var>;

/// What `$response.body(…)` chips show: the workspace's request names and,
/// per call, its value in the source's latest response (`None`: none yet).
#[derive(Default)]
pub struct Refs {
    pub names: HashMap<String, String>,
    pub values: HashMap<String, Option<String>>,
}

/// A response call's source request id and the field it reads, as shown:
/// the selector without `$.`, `body` for the whole body.
pub fn response_ref(expr: &str) -> Option<(String, String)> {
    let args = response_args(expr)?;
    let arg = |k: &str| args.iter().find(|(n, _)| *n == k).map(|(_, v)| v.as_str());
    let sel = arg("selector").unwrap_or("");
    let field = sel
        .strip_prefix("$.")
        .or_else(|| sel.strip_prefix('$'))
        .unwrap_or(sel);
    let field = if field.is_empty() { "body" } else { field };
    Some((arg("requestId")?.to_string(), field.to_string()))
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Defined,
    Missing,
    /// `{{ $fake.email() }}` and friends — evaluated at send time.
    Function,
}

/// Unknown functions are `Missing`: the engine sends them verbatim. A variable
/// still holding an unresolved response call (no stored response yet) is a
/// `Function`: the send fetches it.
pub fn kind(expr: &str, vars: &Vars) -> Kind {
    let pending = |v: &Var| {
        spans(&v.value)
            .iter()
            .any(|(_, e)| response_args(e).is_some())
    };
    if funcs::lookup(expr).is_some() || response_args(expr).is_some() {
        Kind::Function
    } else {
        // Not only identifiers: the engine also sends imported names like
        // `api-gateway` when the env defines them.
        match vars.get(expr) {
            Some(v) if pending(v) => Kind::Function,
            Some(_) => Kind::Defined,
            None => Kind::Missing,
        }
    }
}

/// Every single-line `{{ expr }}` in `text`: byte range of the whole token and
/// the trimmed expression.
pub fn spans(text: &str) -> Vec<(Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find("{{") {
        let start = from + open;
        let Some(close) = text[start + 2..].find("}}") else {
            break;
        };
        let end = start + 2 + close + 2;
        let expr = text[start + 2..end - 2].trim();
        if !expr.is_empty() && !expr.contains('\n') {
            out.push((start..end, expr));
        }
        from = end;
    }
    out
}

/// Atomic chip for one template; the id is the expression, the label hides
/// the braces — and, for a response call, the request id (the chip renders
/// the request's name in its place once known).
pub fn token(source: &str, expr: &str) -> InlineToken {
    let label = response_ref(expr).map_or_else(
        || expr.to_string(),
        |(_, field)| format!("response › {field}"),
    );
    InlineToken::new(expr.to_string(), source.to_string()).with_label(label)
}

/// `text` with each template already chipped, for `set_value`.
pub fn content(text: &str) -> InputContent {
    spans(text)
        .into_iter()
        .fold(InputContent::new(text.to_string()), |acc, (range, expr)| {
            let chip = token(&text[range.clone()], expr);
            acc.clone().with_token(range, chip).unwrap_or(acc)
        })
}

/// `:name` path params before the query (outside `{{ }}`), as byte ranges.
/// With the caret right after a name it's skipped: still being typed.
pub fn path_spans(text: &str, cursor: Option<usize>) -> Vec<Range<usize>> {
    let path_end = text.find(['?', '#']).unwrap_or(text.len());
    let templates = spans(text);
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(colon) = text[from..path_end].find(':') {
        let start = from + colon;
        let end = text[start + 1..]
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .map_or(text.len(), |e| start + 1 + e);
        from = end.max(start + 1);
        if is_identifier(&text[start + 1..end])
            && cursor != Some(end)
            && !templates.iter().any(|(r, _)| r.contains(&start))
        {
            out.push(start..end);
        }
    }
    out
}

/// Chip for a `:name` path param; the id keeps the colon (no template has one).
pub fn path_token(source: &str) -> InlineToken {
    InlineToken::new(source.to_string(), source.to_string()).with_label(source.to_string())
}

/// The URL with templates and path params chipped, for `set_value`.
pub fn url_content(text: &str) -> InputContent {
    path_spans(text, None)
        .into_iter()
        .fold(content(text), |acc, range| {
            let chip = path_token(&text[range.clone()]);
            acc.clone().with_token(range, chip).unwrap_or(acc)
        })
}

/// Distinct expressions referenced across `texts`, first-seen order.
pub fn used<'a>(texts: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for text in texts {
        for (_, expr) in spans(text) {
            if !out.iter().any(|e| e == expr) {
                out.push(expr.to_string());
            }
        }
    }
    out
}

pub fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '$')
}

/// The unfinished `{{ prefix` the cursor sits in: byte offset of the `{{`
/// and the name typed so far.
pub fn open_template(text: &str, cursor: usize) -> Option<(usize, &str)> {
    let before = text.get(..cursor)?;
    let start = before.rfind("{{")?;
    let prefix = before[start + 2..].trim_start();
    prefix.chars().all(is_name_char).then_some((start, prefix))
}

/// A bare `$prefix` right before the cursor (functions without typing `{{`):
/// byte offset of the `$` and the text typed so far, `$` included.
pub fn open_function(text: &str, cursor: usize) -> Option<(usize, &str)> {
    let before = text.get(..cursor)?;
    let start = before.rfind('$')?;
    let typed = &before[start..];
    typed[1..]
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.')
        .then_some((start, typed))
}

/// Names containing `prefix` (any case), those starting with it first.
pub fn suggest<'a>(vars: &'a Vars, prefix: &str) -> Vec<(&'a String, &'a Var)> {
    let p = prefix.to_lowercase();
    let mut out: Vec<_> = vars
        .iter()
        .filter(|(k, _)| k.to_lowercase().contains(&p))
        .collect();
    out.sort_by_key(|(k, _)| !k.to_lowercase().starts_with(&p));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_cover_whole_tokens_and_skip_broken_ones() {
        let text = "{{ BASE }}/a/{{TOKEN}}?x={{ uuid.v4() }}&y={{ }}&z={{ open";
        let got: Vec<_> = spans(text)
            .into_iter()
            .map(|(r, e)| (&text[r], e))
            .collect();
        assert_eq!(
            got,
            [
                ("{{ BASE }}", "BASE"),
                ("{{TOKEN}}", "TOKEN"),
                ("{{ uuid.v4() }}", "uuid.v4()")
            ]
        );
    }

    #[test]
    fn content_chips_every_template_and_keeps_text() {
        let c = content("é {{ A }} {{ A }}");
        assert_eq!(c.text().as_ref(), "é {{ A }} {{ A }}");
        assert_eq!(c.tokens().len(), 2);
        assert_eq!(c.tokens()[0].token().label().as_ref(), "A");
    }

    #[test]
    fn kind_and_used() {
        let mut vars = Vars::new();
        let var = Var {
            raw: "x".into(),
            value: "x".into(),
            source: Source::Global,
            secret: false,
        };
        let pending = Var {
            value: r#"{{ $response.body(requestId="r", selector="$.t") }}"#.into(),
            ..var.clone()
        };
        vars.insert("A".into(), var.clone());
        vars.insert("api-gateway".into(), var);
        vars.insert("TOKEN".into(), pending);
        assert_eq!(kind("A", &vars), Kind::Defined);
        assert_eq!(kind("B", &vars), Kind::Missing);
        assert_eq!(kind("$uuid.v4()", &vars), Kind::Function);
        assert_eq!(kind("uuid.v4()", &vars), Kind::Missing);
        assert_eq!(kind("api-gateway", &vars), Kind::Defined);
        assert_eq!(kind("api-portal", &vars), Kind::Missing);
        assert_eq!(kind("nope.fn()", &vars), Kind::Missing);
        assert_eq!(kind("TOKEN", &vars), Kind::Function);
        assert_eq!(used(["{{ A }}{{B}}", "{{ A }}"]), ["A", "B"]);
    }

    #[test]
    fn origin_follows_the_winning_layer() {
        let var = |source| Var {
            raw: String::new(),
            value: String::new(),
            source,
            secret: false,
        };
        let folder = Source::Folder {
            name: "f".into(),
            id: "fid".into(),
        };
        let vars: Vars = [
            ("F".to_string(), var(folder)),
            ("G".to_string(), var(Source::Global)),
            ("E".to_string(), var(Source::Env("dev".into()))),
            ("S".to_string(), var(Source::System)),
        ]
        .into();
        let env = |global, var: &str| {
            Some(Origin::Env {
                global,
                var: var.into(),
            })
        };
        let folder = Some(Origin::Folder {
            id: "fid".into(),
            var: "F".into(),
        });
        assert_eq!(origin("F", &vars), folder);
        assert_eq!(origin("G", &vars), env(true, "G"));
        assert_eq!(origin("E", &vars), env(false, "E"));
        assert_eq!(origin("NEW", &vars), env(false, "NEW"));
        assert_eq!(origin("S", &vars), None);
        assert_eq!(origin("$fake.email()", &vars), None);
        assert_eq!(
            origin(":code", &vars),
            Some(Origin::PathParam("code".into()))
        );
        assert_eq!(
            origin(r#"response.body(requestId="r1", selector="$.t")"#, &vars),
            Some(Origin::Request("r1".into()))
        );
        assert_eq!(
            response_ref(r#"$response.body(requestId="r1", selector="$")"#),
            Some(("r1".into(), "body".into()))
        );
        assert_eq!(
            response_ref(r#"$response.body(requestId="r1", selector="$.data.token")"#),
            Some(("r1".into(), "data.token".into()))
        );
    }

    #[test]
    fn open_template_and_suggest() {
        assert_eq!(open_template("x {{ BA", 7), Some((2, "BA")));
        assert_eq!(open_template("x {{", 4), Some((2, "")));
        assert_eq!(open_template("{{ A }} b", 9), None);
        assert_eq!(open_template("{{ A b", 6), None);
        assert_eq!(open_template("{{ BA }}", 5), Some((0, "BA")));
        assert_eq!(open_function("x=$fa", 5), Some((2, "$fa")));
        assert_eq!(open_function("$", 1), Some((0, "$")));
        assert_eq!(open_function("$fake.email() x", 15), None);
        let var = Var {
            raw: String::new(),
            value: String::new(),
            source: Source::Global,
            secret: false,
        };
        let vars: Vars = ["API_BASE", "BASE", "TOKEN"]
            .into_iter()
            .map(|k| (k.to_string(), var.clone()))
            .collect();
        let names: Vec<_> = suggest(&vars, "ba")
            .into_iter()
            .map(|(k, _)| k.as_str())
            .collect();
        assert_eq!(names, ["BASE", "API_BASE"]);
    }

    #[test]
    fn path_spans_skip_ports_query_templates_and_typing() {
        let url = "http://h:8080/{{ a:b }}/:id/x/:org_id?q=:no";
        let names: Vec<_> = path_spans(url, None).into_iter().map(|r| &url[r]).collect();
        assert_eq!(names, [":id", ":org_id"]);
        // Caret right after `:org_id`: still typing it.
        let at = url.find('?').unwrap();
        let names: Vec<_> = path_spans(url, Some(at))
            .into_iter()
            .map(|r| &url[r])
            .collect();
        assert_eq!(names, [":id"]);
    }
}
