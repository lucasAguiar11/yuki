//! `{{ … }}` template variables: where they sit in text, what they resolve to,
//! and which layer defines them.

use std::{collections::BTreeMap, ops::Range};

use gpui_kit::component::input::{InlineToken, InputContent};
use yuki_mcp::resolve::is_identifier;

/// The layer a variable comes from, strongest first.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Folder(String),
    Env(String),
    Global,
    System,
}

impl Source {
    pub fn label(&self) -> String {
        match self {
            Source::Folder(name) => format!("pasta · {name}"),
            Source::Env(name) => name.clone(),
            Source::Global => "global".into(),
            Source::System => "sistema".into(),
        }
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

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Defined,
    Missing,
    /// `{{ uuid.v4() }}` and friends — evaluated at send time.
    Function,
}

pub fn kind(expr: &str, vars: &Vars) -> Kind {
    if !is_identifier(expr) {
        Kind::Function
    } else if vars.contains_key(expr) {
        Kind::Defined
    } else {
        Kind::Missing
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
/// the braces.
pub fn token(source: &str, expr: &str) -> InlineToken {
    InlineToken::new(expr.to_string(), source.to_string()).with_label(expr.to_string())
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
        vars.insert("A".into(), var);
        assert_eq!(kind("A", &vars), Kind::Defined);
        assert_eq!(kind("B", &vars), Kind::Missing);
        assert_eq!(kind("uuid.v4()", &vars), Kind::Function);
        assert_eq!(used(["{{ A }}{{B}}", "{{ A }}"]), ["A", "B"]);
    }
}
