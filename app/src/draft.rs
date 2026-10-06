//! Text <-> model mapping for the request editor. Headers edit as plain text,
//! one `Name: value` per line; a leading `# ` disables the row.

use yuki_core::{new_id, BodyKind, HttpRequest, RequestBody, RequestParameter};

pub fn headers_to_text(headers: &[RequestParameter]) -> String {
    headers
        .iter()
        .map(|h| {
            let prefix = if h.enabled { "" } else { "# " };
            format!("{prefix}{}: {}", h.name, h.value)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse header text, reusing the id of the row at the same position when the
/// name matches — fresh ids on every save would be a phantom git diff.
pub fn text_to_headers(text: &str, prev: &[RequestParameter]) -> Vec<RequestParameter> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .enumerate()
        .map(|(i, line)| {
            let (enabled, line) = match line.strip_prefix('#') {
                Some(rest) => (false, rest.trim_start()),
                None => (true, line),
            };
            let (name, value) = line.split_once(':').unwrap_or((line, ""));
            let (name, value) = (name.trim().to_string(), value.trim().to_string());
            let id = prev
                .get(i)
                .filter(|p| p.name == name)
                .map_or_else(new_id, |p| p.id.clone());
            RequestParameter {
                id,
                name,
                value,
                enabled,
            }
        })
        .collect()
}

/// Bodies the text editor owns. Form/multipart/binary keep their stored shape.
pub fn body_is_text(body: Option<&RequestBody>) -> bool {
    !matches!(
        body.map(|b| &b.kind),
        Some(BodyKind::FormUrlEncoded | BodyKind::Multipart | BodyKind::Binary)
    )
}

pub fn apply_body(prev: Option<&RequestBody>, text: &str) -> Option<RequestBody> {
    match prev {
        Some(b) if !body_is_text(Some(b)) => Some(b.clone()),
        Some(b) if b.kind != BodyKind::None || !text.is_empty() => Some(RequestBody {
            kind: if b.kind == BodyKind::None {
                BodyKind::Json
            } else {
                b.kind.clone()
            },
            text: text.to_string(),
            ..b.clone()
        }),
        Some(b) => Some(b.clone()),
        None if text.is_empty() => None,
        None => Some(RequestBody {
            kind: BodyKind::Json,
            text: text.to_string(),
            ..Default::default()
        }),
    }
}

/// The request as the editor would save it.
pub fn build(
    saved: &HttpRequest,
    method: &str,
    url: &str,
    headers: &str,
    body: &str,
) -> HttpRequest {
    HttpRequest {
        method: method.to_string(),
        url: url.to_string(),
        headers: text_to_headers(headers, &saved.headers),
        body: apply_body(saved.body.as_ref(), body),
        ..saved.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn param(id: &str, name: &str, value: &str, enabled: bool) -> RequestParameter {
        RequestParameter {
            id: id.into(),
            name: name.into(),
            value: value.into(),
            enabled,
        }
    }

    #[test]
    fn headers_round_trip_keeps_ids_and_disabled_rows() {
        let prev = vec![
            param("a", "Accept", "*/*", true),
            param("b", "X-Debug", "1", false),
        ];
        let text = headers_to_text(&prev);
        assert_eq!(text, "Accept: */*\n# X-Debug: 1");
        assert_eq!(text_to_headers(&text, &prev), prev);
    }

    #[test]
    fn renamed_header_gets_new_id_and_value_keeps_colons() {
        let prev = vec![param("a", "Accept", "*/*", true)];
        let out = text_to_headers("Host: localhost:8080", &prev);
        assert_ne!(out[0].id, "a");
        assert_eq!(out[0].value, "localhost:8080");
    }

    #[test]
    fn form_bodies_are_preserved_and_empty_text_stays_none() {
        let form = RequestBody {
            kind: BodyKind::FormUrlEncoded,
            ..Default::default()
        };
        assert_eq!(apply_body(Some(&form), "ignored"), Some(form));
        assert_eq!(apply_body(None, ""), None);
        assert_eq!(apply_body(None, "{}").unwrap().kind, BodyKind::Json);
    }
}
