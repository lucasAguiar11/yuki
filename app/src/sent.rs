//! Response "timeline" tab: what actually went out (resolved URL, body) and
//! the engine's event log — request line, wire headers, timings — `curl -v`
//! style.

use gpui_kit::base::SelectableText;
use gpui_kit::component::{v_flex, ActiveTheme as _};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_core::{BodyKind, HttpRequest, HttpResponse};

use crate::var_view::MASK;

/// Credential headers. Shown as `Bearer ••••••••a1b2`: a screen share never
/// leaks them, yet you can still tell which token went out.
const SECRET_HEADERS: [&str; 5] = [
    "authorization",
    "proxy-authorization",
    "cookie",
    "x-api-key",
    "api-key",
];

pub fn mask_header(name: &str, value: &str) -> String {
    let name = name.trim().to_ascii_lowercase();
    if !SECRET_HEADERS.contains(&name.as_str()) {
        return value.to_string();
    }
    // Keep the scheme (`Bearer`, `Basic`): it tells which auth ran.
    let (scheme, secret) = match value.split_once(' ') {
        Some((s, v)) if name.ends_with("authorization") => (format!("{s} "), v),
        _ => (String::new(), value),
    };
    let n = secret.chars().count();
    let tail: String = if n >= 12 {
        secret.chars().skip(n - 4).collect()
    } else {
        String::new()
    };
    format!("{scheme}{MASK}{tail}")
}

/// Header-shaped event lines (`Name: value`) go through `mask_header`.
fn mask_line(line: &str) -> String {
    match line.split_once(": ") {
        Some((name, value)) => format!("{name}: {}", mask_header(name, value)),
        None => line.to_string(),
    }
}

/// The body as sent, `None` when there was none.
fn body_text(req: &HttpRequest) -> Option<String> {
    let b = req.body.as_ref()?;
    let text = match b.kind {
        BodyKind::None => return None,
        BodyKind::Graphql => b.graphql_payload(),
        BodyKind::FormUrlEncoded | BodyKind::Multipart => b
            .fields
            .iter()
            .flatten()
            .filter(|f| f.enabled)
            .map(|f| {
                let at = if f.is_file { "@" } else { "" };
                format!("{}: {at}{}", f.name, f.value)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        BodyKind::Binary => format!("@{}", b.file_path.as_deref().unwrap_or("")),
        _ => b.text.clone(),
    };
    (!text.trim().is_empty()).then_some(text)
}

fn ms(at: f64) -> String {
    if at < 10. {
        format!("{at:.1} ms")
    } else {
        format!("{at:.0} ms")
    }
}

/// The timeline as plain lines (what ⌘C / "copy" yield), each flagged as
/// wire traffic (bright) or an engine note (muted). `curl -v` shaped.
fn lines(resp: &HttpResponse, sent: Option<&HttpRequest>) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    if let Some(req) = sent {
        out.push((format!("{} {}", req.method, req.url), true));
    }
    // A stamp only when it changes: bursts at the same instant read as a block.
    let mut prev = String::new();
    for ev in &resp.events {
        let at = ms(ev.at_ms);
        let stamp = if at == prev { "" } else { at.as_str() };
        let (mark, wire) = match ev.kind.as_str() {
            "send" => (">", true),
            "recv" => ("<", true),
            "done" => ("✓", false),
            "error" => ("✕", false),
            _ => ("*", false),
        };
        out.push((format!("{stamp:>8} {mark} {}", mask_line(&ev.text)), wire));
        prev = at;
    }
    out
}

/// The whole timeline as text, for "copy all".
pub fn copy_text(resp: &HttpResponse, sent: Option<&HttpRequest>) -> String {
    let mut out: Vec<String> = lines(resp, sent).into_iter().map(|(l, _)| l).collect();
    out.extend(sent.and_then(body_text).map(|b| format!("\n{b}")));
    out.join("\n")
}

pub fn render(
    resp: &HttpResponse,
    sent: Option<&HttpRequest>,
    focus: &FocusHandle,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let (fg, muted) = (theme.foreground, theme.muted_foreground);
    let lines = lines(resp, sent);
    let body = sent.and_then(body_text);

    // Every line is its own selectable run; document order keeps a drag
    // across lines (and ⌘C) in reading order.
    let run = |ix: usize, text: String| {
        SelectableText::new(("timeline", ix), text).document_order(ix as u64)
    };
    let n = lines.len();
    let rows = lines.into_iter().enumerate().map(|(ix, (text, wire))| {
        div()
            .text_color(if wire { fg } else { muted })
            .when(ix == 0 && sent.is_some(), |d| d.text_sm().pb_2())
            .child(run(ix, text))
    });

    let focus_on_click = focus.clone();
    let mut col = v_flex()
        .id("timeline")
        // Focused on click so ⌘C reaches the window selection instead of
        // the URL bar, which otherwise holds focus.
        .track_focus(focus)
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            focus_on_click.focus(window, cx)
        })
        .size_full()
        .overflow_y_scroll()
        .px_4()
        .py_3()
        .gap_0p5()
        .text_xs()
        .font_family(theme.mono_font_family.clone())
        .children(rows.collect::<Vec<_>>());
    if resp.events.is_empty() {
        col = col.child(
            div()
                .text_color(muted)
                .child(t!("no events recorded for this send")),
        );
    }
    if let Some(body) = body {
        col = col
            .child(div().pt_3().text_color(muted).child(t!("sent body")))
            .child(
                div()
                    .p_2()
                    .rounded(theme.radius)
                    .bg(theme.sidebar)
                    .text_color(fg)
                    .child(run(n, body)),
            );
    }
    col.into_any_element()
}

#[cfg(test)]
mod tests {
    // Not `super::*`: the gpui glob would shadow `#[test]` with its own macro.
    use super::{mask_header, mask_line, MASK};

    #[test]
    fn masks_credentials_but_keeps_scheme_and_tail() {
        let token = "Bearer abcdefghijklmnop";
        assert_eq!(
            mask_header("Authorization", token),
            format!("Bearer {MASK}mnop")
        );
        assert_eq!(
            mask_header("cookie", "sid=1; theme=dark"),
            format!("{MASK}dark")
        );
        assert_eq!(mask_header("x-api-key", "short"), MASK);
        assert_eq!(mask_header("Accept", "a b"), "a b");
        assert_eq!(
            mask_line("authorization: Basic dXNlcjpwYXNzd29yZA=="),
            format!("authorization: Basic {MASK}ZA==")
        );
        assert_eq!(mask_line("GET /a HTTP/1.1"), "GET /a HTTP/1.1");
    }
}
