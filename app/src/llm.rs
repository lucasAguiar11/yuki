//! "Copy for LLM": one request — as sent, its response, timings and notes —
//! as Markdown to paste into a chat. Credentials masked like the timeline;
//! big JSON keeps its shape (arrays cut to a few items), binary is described.

use gpui_kit::*;
use serde_json::Value;
use yuki_core::{HttpRequest, HttpResponse};

use crate::{sent, tree, ui, workspace::Yuki};

/// Array items kept per JSON array; the rest becomes `"… N more"`.
const ARRAY_ITEMS: usize = 3;
/// Text bodies past this many chars are cut (with a note).
const TEXT_CHARS: usize = 8000;

impl Yuki {
    /// Request `id` for an LLM, on the clipboard. The response is the one its
    /// tab shows, else the latest stored; never sent → the request resolved
    /// as it would go out.
    pub(crate) fn copy_for_llm(&mut self, id: SharedString, cx: &mut Context<Self>) {
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
        let shown = tab.and_then(|t| {
            let e = t.read(cx);
            match &e.response {
                Some(Ok(r)) => Some((r.clone(), e.sent.clone())),
                _ => None,
            }
        });
        let notes = tab.map(|t| t.read(cx).notes_text(cx));
        let data = self.data.as_ref();
        let folder = data
            .map(|d| tree::folder_path(&d.folders, req.folder_id.as_deref()).join(" / "))
            .unwrap_or_default();
        let env_id = self.env_id(cx);
        let env = env_id.as_ref().and_then(|id| {
            data?
                .environments
                .iter()
                .find(|e| &e.id == id)
                .map(|e| e.name.clone())
        });
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let (ws, rid) = (req.workspace_id.clone(), req.id.clone());
            let stored = {
                let backend = backend.clone();
                cx.background_spawn(async move {
                    let notes =
                        notes.unwrap_or_else(|| backend.notes(&ws, &rid).unwrap_or_default());
                    let latest = shown.or_else(|| {
                        let first = backend.history(&ws, &rid).into_iter().next()?;
                        backend.stored_response(&ws, &rid, &first.id).ok()
                    });
                    (notes, latest)
                })
                .await
            };
            let (notes, latest) = stored;
            let (resp, mut sent) = latest.map_or((None, None), |(r, s)| (Some(r), s));
            if sent.is_none() {
                sent = backend.resolve(req.clone(), env_id.clone()).await.ok();
            }
            let text = context(&LlmContext {
                method: &req.method,
                name: &req.name,
                ids: (&req.workspace_id, &req.id, env_id.as_deref()),
                folder: &folder,
                env: env.as_deref(),
                sent: sent.as_ref(),
                resp: resp.as_ref(),
                notes: &notes,
            });
            _ = this.update(cx, |this, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                this.status = t!("copied for LLM").into_owned().into();
                cx.notify();
            });
        })
        .detach();
    }
}

pub struct LlmContext<'a> {
    pub method: &'a str,
    pub name: &'a str,
    /// Workspace, request and environment ids: what the MCP tools take.
    pub ids: (&'a str, &'a str, Option<&'a str>),
    /// `Stone / OM`; empty at the root.
    pub folder: &'a str,
    pub env: Option<&'a str>,
    /// The request as sent (resolved); `None` if it never was.
    pub sent: Option<&'a HttpRequest>,
    pub resp: Option<&'a HttpResponse>,
    pub notes: &'a str,
}

pub fn context(c: &LlmContext) -> String {
    let mut out = vec![format!("## {} {}", c.method, c.name)];
    let mut place = Vec::new();
    if !c.folder.is_empty() {
        place.push(format!("folder: {}", c.folder));
    }
    if let Some(env) = c.env {
        place.push(format!("environment: {env}"));
    }
    if !place.is_empty() {
        out.push(place.join(" · "));
    }
    let (ws, req, env) = c.ids;
    out.push(format!("ids: workspace `{ws}` · request `{req}`"));
    if let Some(req) = c.sent {
        out.push(String::new());
        out.push(if c.resp.is_some() {
            "### request (as sent)".into()
        } else {
            "### request (resolved, not sent yet)".into()
        });
        out.push(fence("http", &request_head(req)));
        if let Some(body) = sent::body_text(req) {
            out.push(fence(lang(&body), &trim(&body)));
        }
    }
    match c.resp {
        Some(resp) => {
            out.push(String::new());
            out.push(format!(
                "### response: {} {} · {:.0} ms · {}",
                resp.status,
                resp.status_text,
                resp.timing.total_ms,
                ui::human_bytes(resp.body_size)
            ));
            if let Some(t) = timing(resp) {
                out.push(format!("timing: {t}"));
            }
            out.push(fence("http", &headers(resp)));
            out.push(response_body(resp));
        }
        None => {
            out.push(String::new());
            out.push("### response: not sent yet".into());
        }
    }
    if !c.notes.trim().is_empty() {
        out.push(String::new());
        out.push("### notes".into());
        out.push(c.notes.trim().to_string());
    }
    out.push(String::new());
    out.push(mcp(ws, req, env));
    out.join("\n")
}

/// How an agent wired to Yuki's MCP reads or re-sends this same request.
fn mcp(ws: &str, req: &str, env: Option<&str>) -> String {
    let ids = serde_json::json!({ "workspaceId": ws, "requestId": req });
    let mut send = ids.clone();
    if let Some(env) = env {
        send["environmentId"] = env.into();
    }
    format!(
        "### Yuki MCP\nAn agent connected to Yuki's MCP server can read or re-send this request:\n{}",
        fence(
            "",
            &format!("request.get   {ids}\nrequest.send  {send}\nresponse.list {ids}")
        )
    )
}

fn fence(lang: &str, text: &str) -> String {
    format!("```{lang}\n{}\n```", text.trim_end())
}

fn lang(text: &str) -> &'static str {
    match text.trim_start().chars().next() {
        Some('{' | '[') if serde_json::from_str::<Value>(text).is_ok() => "json",
        Some('<') => "xml",
        _ => "",
    }
}

fn request_head(req: &HttpRequest) -> String {
    let mut lines = vec![format!("{} {}", req.method, req.url)];
    lines.extend(
        req.headers
            .iter()
            .filter(|h| h.enabled && !h.name.trim().is_empty())
            .map(|h| format!("{}: {}", h.name.trim(), mask(&h.name, &h.value))),
    );
    lines.join("\n")
}

fn headers(resp: &HttpResponse) -> String {
    resp.headers
        .iter()
        .map(|h| format!("{}: {}", h.name, mask(&h.name, &h.value)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The timeline's masking, plus `Set-Cookie` (a session handed back).
fn mask(name: &str, value: &str) -> String {
    if name.trim().eq_ignore_ascii_case("set-cookie") {
        return sent::mask_header("cookie", value);
    }
    sent::mask_header(name, value)
}

/// `dns 4 ms · connection 12 ms · tls 30 ms · server wait 900 ms · download 8 ms`.
fn timing(resp: &HttpResponse) -> Option<String> {
    let t = &resp.timing;
    let parts: Vec<String> = [
        ("dns", t.dns_ms),
        ("connection", t.connect_ms),
        ("tls", t.tls_ms),
        ("server wait", t.first_byte_ms),
        ("download", t.download_ms),
    ]
    .iter()
    .filter(|(_, ms)| *ms >= 0.5)
    .map(|(name, ms)| format!("{name} {ms:.0} ms"))
    .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn response_body(resp: &HttpResponse) -> String {
    if !resp.sse_frames.is_empty() {
        let frames: Vec<String> = resp
            .sse_frames
            .iter()
            .take(20)
            .map(|f| format!("[{}] {}", f.event.as_deref().unwrap_or("message"), f.data))
            .collect();
        return fence("", &frames.join("\n"));
    }
    if !resp.body_is_text {
        let ct = crate::binary::content_type(resp);
        return format!("body: ‹{ct}, {}›", ui::human_bytes(resp.body_size));
    }
    if resp.body.trim().is_empty() {
        return "body: (empty)".into();
    }
    fence(lang(&resp.body), &trim(&resp.body))
}

/// JSON pretty and cut by array length; other text cut by length.
fn trim(text: &str) -> String {
    if let Ok(mut v) = serde_json::from_str::<Value>(text) {
        cut_arrays(&mut v);
        return serde_json::to_string_pretty(&v).unwrap_or_else(|_| text.to_string());
    }
    match text.char_indices().nth(TEXT_CHARS) {
        Some((at, _)) => format!(
            "{}\n… ({} more chars)",
            &text[..at],
            text[at..].chars().count()
        ),
        None => text.to_string(),
    }
}

fn cut_arrays(v: &mut Value) {
    match v {
        Value::Array(items) => {
            let extra = items.len().saturating_sub(ARRAY_ITEMS);
            items.truncate(ARRAY_ITEMS);
            items.iter_mut().for_each(cut_arrays);
            if extra > 0 {
                items.push(Value::String(format!("… {extra} more")));
            }
        }
        Value::Object(map) => map.values_mut().for_each(cut_arrays),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{mask, trim};

    #[test]
    fn json_keeps_shape_and_cuts_long_arrays() {
        let body = r#"{"data":[1,2,3,4,5],"nested":{"x":[{"a":[1,2,3,4]}]}}"#;
        let out: serde_json::Value = serde_json::from_str(&trim(body)).unwrap();
        assert_eq!(out["data"], serde_json::json!([1, 2, 3, "… 2 more"]));
        assert_eq!(out["nested"]["x"][0]["a"][3], "… 1 more");
        assert_eq!(trim("short text"), "short text");
        assert!(trim(&"x".repeat(9000)).ends_with("… (1000 more chars)"));
    }

    #[test]
    fn masks_credentials_and_cookies() {
        let token = "Bearer eyJhbGciOiJIUzI1NiJ9.payload.sig1234";
        assert!(!mask("Authorization", token).contains("payload"));
        assert!(mask("Authorization", token).starts_with("Bearer "));
        assert!(!mask("Set-Cookie", "session=abcdefghijklmnop; Path=/").contains("abcdefgh"));
        assert_eq!(mask("Content-Type", "application/json"), "application/json");
    }
}
