//! Editor state -> model mapping: what the request would look like if saved.

use yuki_core::{BodyField, BodyKind, HttpRequest, RequestBody, RequestParameter};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untouched_multiline_cell_keeps_stored_value() {
        let stored = "l1\r\nl2\nl3";
        assert_eq!(one_line(stored), "l1 l2 l3");
        assert_eq!(cell_value("l1 l2 l3", stored), stored);
        assert_eq!(cell_value("l1 l2", stored), "l1 l2");
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
}
