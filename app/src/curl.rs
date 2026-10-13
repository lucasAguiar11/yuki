//! "Copy as cURL": a request resolved by `ApiBackend::resolve_http` (literal
//! URL/headers/body) as a command that reproduces it. Body handling mirrors
//! `yuki-http`'s `attach_body`, including its default Content-Type.

use gpui_kit::*;
use yuki_core::{AuthConfig, BodyKind, HttpRequest};

use crate::workspace::Yuki;

impl Yuki {
    /// Copies request `id` as cURL — from its open tab's draft when there is
    /// one, so unsaved edits count — resolved against the selected env.
    pub(crate) fn copy_as_curl(&mut self, id: SharedString, cx: &mut Context<Self>) {
        let tab = self
            .tabs
            .iter()
            .find(|t| t.read(cx).saved.id == id.as_ref());
        let Some(req) = tab
            .map(|t| t.read(cx).draft(cx))
            .or_else(|| self.request(&id).cloned())
        else {
            return;
        };
        let (backend, env) = (self.backend.clone(), self.env_id(cx));
        cx.spawn(async move |this, cx| {
            let result = backend.resolve(req, env).await;
            _ = this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(r) => {
                        cx.write_to_clipboard(ClipboardItem::new_string(curl(&r)));
                        t!("copied as cURL").into_owned().into()
                    }
                    Err(e) => t!("failed: %{e}", e = e).into_owned().into(),
                };
                cx.notify();
            });
        })
        .detach();
    }
}

/// POSIX single-quoted: nothing inside expands.
fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn curl(req: &HttpRequest) -> String {
    let mut args = vec![match req.method.as_str() {
        "GET" => format!("curl {}", q(&req.url)),
        m => format!("curl -X {m} {}", q(&req.url)),
    }];
    let headers: Vec<_> = req
        .headers
        .iter()
        .filter(|h| h.enabled && !h.name.trim().is_empty())
        .collect();
    let user_ct = headers
        .iter()
        .any(|h| h.name.trim().eq_ignore_ascii_case("content-type"));
    for h in headers {
        args.push(format!(
            "-H {}",
            q(&format!("{}: {}", h.name.trim(), h.value))
        ));
    }
    let ct = |ct: &str, args: &mut Vec<String>| {
        if !user_ct {
            args.push(format!("-H {}", q(&format!("Content-Type: {ct}"))));
        }
    };

    if let Some(b) = req.body.as_ref() {
        let fields = b.fields.iter().flatten();
        let fields = fields.filter(|f| f.enabled && !f.name.trim().is_empty());
        match b.kind {
            BodyKind::None => {}
            BodyKind::Json
            | BodyKind::Xml
            | BodyKind::Html
            | BodyKind::Text
            | BodyKind::Graphql
                if b.text.is_empty() => {}
            BodyKind::Json | BodyKind::Graphql => {
                ct("application/json", &mut args);
                let data = match b.kind {
                    BodyKind::Graphql => b.graphql_payload(),
                    _ => b.text.clone(),
                };
                args.push(format!("--data-raw {}", q(&data)));
            }
            BodyKind::Xml | BodyKind::Html | BodyKind::Text => {
                ct(
                    match b.kind {
                        BodyKind::Xml => "application/xml",
                        BodyKind::Html => "text/html",
                        _ => "text/plain",
                    },
                    &mut args,
                );
                args.push(format!("--data-raw {}", q(&b.text)));
            }
            BodyKind::FormUrlEncoded => args
                .extend(fields.map(|f| {
                    format!("--data-urlencode {}", q(&format!("{}={}", f.name, f.value)))
                })),
            // `--form-string` for text: `-F` would read a value starting with
            // `@` or `<` from a file.
            BodyKind::Multipart => args.extend(fields.map(|f| {
                if f.is_file {
                    let ty = f.content_type.as_deref().map(|t| format!(";type={t}"));
                    let part = format!("{}=@{}{}", f.name, f.value, ty.unwrap_or_default());
                    format!("-F {}", q(&part))
                } else {
                    format!("--form-string {}", q(&format!("{}={}", f.name, f.value)))
                }
            })),
            BodyKind::Binary => {
                if let Some(path) = b.file_path.as_deref().filter(|p| !p.trim().is_empty()) {
                    let t = b.content_type.as_deref().filter(|c| !c.trim().is_empty());
                    ct(t.unwrap_or("application/octet-stream"), &mut args);
                    args.push(format!("--data-binary {}", q(&format!("@{path}"))));
                }
            }
        }
    }

    // Static auth is already a header; the executor signs these at send time.
    if req.auth.is_active() {
        match &req.auth {
            AuthConfig::Digest {
                username, password, ..
            } => args.push(format!(
                "--digest -u {}",
                q(&format!("{username}:{password}"))
            )),
            AuthConfig::Ntlm {
                username, password, ..
            } => args.push(format!(
                "--ntlm -u {}",
                q(&format!("{username}:{password}"))
            )),
            AuthConfig::AwsSigV4 {
                access_key,
                secret_key,
                session_token,
                region,
                service,
                ..
            } => {
                args.push(format!(
                    "--aws-sigv4 {}",
                    q(&format!("aws:amz:{region}:{service}"))
                ));
                args.push(format!("-u {}", q(&format!("{access_key}:{secret_key}"))));
                if !session_token.is_empty() {
                    args.push(format!(
                        "-H {}",
                        q(&format!("X-Amz-Security-Token: {session_token}"))
                    ));
                }
            }
            // ponytail: OAuth 1.0 has no curl flag; the command goes unsigned.
            _ => {}
        }
    }
    args.join(" \\\n  ")
}

#[cfg(test)]
mod tests {
    use super::curl;
    use yuki_core::HttpRequest;

    fn req(json: serde_json::Value) -> HttpRequest {
        let mut base = serde_json::json!({
            "id": "r", "type": "http_request", "model": "http_request", "workspaceId": "w",
            "folderId": null, "method": "GET", "name": "r", "url": "https://x.dev/a?b=1",
            "createdAt": "", "updatedAt": ""
        });
        base.as_object_mut()
            .unwrap()
            .extend(json.as_object().unwrap().clone());
        serde_json::from_value(base).unwrap()
    }

    #[test]
    fn json_body_quotes_and_default_content_type() {
        let r = req(serde_json::json!({
            "method": "POST",
            "headers": [
                {"id": "1", "name": "X-A", "value": "it's", "enabled": true},
                {"id": "2", "name": "X-Off", "value": "no", "enabled": false}
            ],
            "body": {"kind": "json", "text": "{\"a\":\"o'k\"}"}
        }));
        assert_eq!(
            curl(&r),
            "curl -X POST 'https://x.dev/a?b=1' \\\n  -H 'X-A: it'\\''s' \\\n  \
             -H 'Content-Type: application/json' \\\n  --data-raw '{\"a\":\"o'\\''k\"}'"
        );
    }

    #[test]
    fn user_content_type_wins_and_multipart_text_is_literal() {
        let r = req(serde_json::json!({
            "method": "POST",
            "headers": [{"id": "1", "name": "content-type", "value": "x/y", "enabled": true}],
            "body": {"kind": "multipart", "text": "", "fields": [
                {"id": "a", "name": "note", "value": "@not-a-file", "enabled": true},
                {"id": "b", "name": "doc", "value": "/tmp/a.pdf", "enabled": true, "isFile": true}
            ]}
        }));
        assert_eq!(
            curl(&r),
            "curl -X POST 'https://x.dev/a?b=1' \\\n  -H 'content-type: x/y' \\\n  \
             --form-string 'note=@not-a-file' \\\n  -F 'doc=@/tmp/a.pdf'"
        );
    }
}
