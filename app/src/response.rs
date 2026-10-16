use gpui_kit::base::text::TextView;
use gpui_kit::component::{h_flex, input::Editor, tooltip::Tooltip, v_flex, ActiveTheme as _};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_core::HttpResponse;

use crate::{
    editor::{RequestEditor, RespTab},
    sent, ui, CaptureField,
};

/// Body text for the read-only viewer + highlighter language.
pub fn body_for_editor(resp: &HttpResponse) -> (String, &'static str) {
    if !resp.sse_frames.is_empty() {
        let frames = resp
            .sse_frames
            .iter()
            .map(|f| format!("[{}] {}", f.event.as_deref().unwrap_or("message"), f.data));
        return (frames.collect::<Vec<_>>().join("\n"), "text");
    }
    if !resp.body_is_text {
        return (
            t!("<binary, %{size}>", size = ui::human_bytes(resp.body_size)).into_owned(),
            "text",
        );
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&resp.body) {
        let pretty = serde_json::to_string_pretty(&v).unwrap_or_else(|_| resp.body.clone());
        return (pretty, "json");
    }
    // XML rides the HTML grammar (same tags/attributes; no XML grammar ships).
    let ct = |s: &str| {
        resp.headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("content-type") && h.value.contains(s))
    };
    if ct("xml") || resp.body.trim_start().starts_with("<?xml") {
        let pretty = crate::xml::pretty_xml(&resp.body).unwrap_or_else(|| resp.body.clone());
        return (pretty, "html");
    }
    (resp.body.clone(), if ct("html") { "html" } else { "text" })
}

fn is_html(resp: &HttpResponse) -> bool {
    resp.body_is_text
        && resp
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("content-type") && h.value.contains("text/html"))
}

/// The tab actually shown: preview falls back to body when the response isn't HTML.
fn shown_tab(editor: &RequestEditor, resp: &HttpResponse) -> RespTab {
    match editor.resp_tab {
        RespTab::Preview if !is_html(resp) => RespTab::Body,
        t => t,
    }
}

pub fn render(editor: &RequestEditor, cx: &mut Context<RequestEditor>) -> AnyElement {
    let theme = cx.theme();
    let content =
        match &editor.response {
            None => empty(editor, cx),
            Some(Err(e)) => v_flex()
                .p_4()
                .gap_2()
                .text_sm()
                .child(div().text_color(theme.red).child(t!("✕ send failed")))
                .child(div().text_color(theme.foreground).child(e.clone()))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(t!("check the URL and the selected environment")),
                )
                .into_any_element(),
            Some(Ok(resp)) => match shown_tab(editor, resp) {
                RespTab::Body => div()
                    .size_full()
                    .pl_2()
                    .key_context("ResponseBody")
                    .on_action(cx.listener(|this, _: &CaptureField, window, cx| {
                        this.start_capture(window, cx)
                    }))
                    .child(
                        Editor::new(&editor.response_body)
                            .appearance(false)
                            .readonly(true)
                            .h_full(),
                    )
                    .into_any_element(),
                RespTab::Preview => TextView::html("resp-html", resp.body.clone())
                    .scrollable(true)
                    .selectable(true)
                    .size_full()
                    .px_4()
                    .into_any_element(),
                RespTab::Headers => headers_table(resp, cx),
                RespTab::Timeline => {
                    sent::render(resp, editor.sent.as_ref(), &editor.timeline_focus, cx)
                }
            },
        };
    v_flex()
        .size_full()
        .child(strip(editor, cx))
        .when(
            match &editor.response {
                Some(Ok(r)) => shown_tab(editor, r),
                _ => editor.resp_tab,
            } == RespTab::Body,
            |r| r.children(editor.render_capture_bar(cx)),
        )
        .child(div().flex_1().min_h_0().child(content))
        .into_any_element()
}

/// `200 OK · 128 ms · 1.0 KB · 14:03:12   body  headers 20  timeline   ●●●●`
fn strip(editor: &RequestEditor, cx: &mut Context<RequestEditor>) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let mut row = h_flex()
        .h(px(30.))
        .px_4()
        .gap_2()
        .text_xs()
        .font_family(theme.mono_font_family.clone())
        .text_color(muted)
        .bg(theme.sidebar)
        .border_b_1()
        .border_color(theme.border);

    // While a send is in flight the strip shows its clock, not the old result.
    if let Some(Ok(resp)) = editor.response.as_ref().filter(|_| !editor.sending) {
        let tab = |id: &'static str, label: String, t: RespTab, cx: &mut Context<RequestEditor>| {
            let active = shown_tab(editor, resp) == t;
            div()
                .id(id)
                .px_1p5()
                .rounded(cx.theme().radius)
                .cursor_pointer()
                .when(active, |s| {
                    s.bg(cx.theme().secondary).text_color(cx.theme().foreground)
                })
                .hover(|s| s.text_color(cx.theme().foreground))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.resp_tab = t;
                    cx.notify();
                }))
        };
        // When it was sent (local time), if history has it.
        let at = editor
            .history
            .iter()
            .find(|s| s.id == resp.response_id)
            .map(|s| ui::clock(&s.recorded_at));
        row = row
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(ui::status_color(resp.status, cx))
                    .child(format!("{} {}", resp.status, resp.status_text)),
            )
            .child(
                div()
                    .id("resp-time")
                    .child(format!("{:.0} ms", resp.timing.total_ms))
                    .tooltip({
                        let label = SharedString::from(timing_label(resp));
                        move |window, cx| Tooltip::new(label.clone()).build(window, cx)
                    }),
            )
            .child(ui::human_bytes(resp.body_size))
            .children(at.map(|t| div().text_color(ui::faint(cx)).child(t)))
            .child(div().w(px(16.)))
            .child(tab("resp-body", "body".into(), RespTab::Body, cx))
            .when(is_html(resp), |r| {
                r.child(tab(
                    "resp-preview",
                    t!("preview").into_owned(),
                    RespTab::Preview,
                    cx,
                ))
            })
            .child(tab(
                "resp-headers",
                format!("headers {}", resp.headers.len()),
                RespTab::Headers,
                cx,
            ))
            .child(tab(
                "resp-timeline",
                "timeline".into(),
                RespTab::Timeline,
                cx,
            ))
            // Outside the timeline itself: clicks there start a selection.
            .when(editor.resp_tab == RespTab::Timeline, |r| {
                let text = sent::copy_text(resp, editor.sent.as_ref());
                let fg = cx.theme().foreground;
                r.child(
                    div()
                        .id("timeline-copy")
                        .px_1p5()
                        .cursor_pointer()
                        .hover(move |s| s.text_color(fg))
                        .child(t!("copy all"))
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
                        }),
                )
            });
    } else {
        row = row.child(if editor.sending {
            t!("sending… %{elapsed}", elapsed = editor.elapsed()).into_owned()
        } else {
            t!("no response").into_owned()
        });
    }
    row.child(div().flex_1()).child(history(editor, cx))
}

/// `server wait 112 ms · download 16 ms` — the phases the client can measure.
fn timing_label(resp: &HttpResponse) -> String {
    let t = &resp.timing;
    let phases = [
        ("dns", t.dns_ms),
        ("connection", t.connect_ms),
        ("tls", t.tls_ms),
        ("server wait", t.first_byte_ms),
        ("download", t.download_ms),
    ];
    let parts: Vec<String> = phases
        .iter()
        .filter(|(_, ms)| *ms >= 0.5)
        .map(|(name, ms)| format!("{} {ms:.0} ms", t!(*name)))
        .collect();
    if parts.is_empty() {
        t!("no timing breakdown").into_owned()
    } else {
        parts.join(" · ")
    }
}

/// Last sends, oldest → newest; click to reopen a stored response.
fn history(editor: &RequestEditor, cx: &mut Context<RequestEditor>) -> impl IntoElement {
    let shown = match &editor.response {
        Some(Ok(r)) => r.response_id.as_str(),
        _ => "",
    };
    let dots = editor.history.iter().take(16).rev().map(|s| {
        let selected = s.id == shown;
        let color = ui::status_color(s.status, cx);
        let label = SharedString::from(format!(
            "{} {} · {:.0} ms · {}",
            s.status,
            s.status_text,
            s.total_ms,
            ui::clock(&s.recorded_at)
        ));
        let id = s.id.clone();
        div()
            .id(SharedString::from(format!("hist-{}", s.id)))
            .size(px(14.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .when(selected, |d| {
                d.border_1().border_color(cx.theme().foreground)
            })
            .child(div().size(px(6.)).rounded_full().bg(color))
            .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
            .on_click(
                cx.listener(move |this, _, window, cx| this.show_history(id.clone(), window, cx)),
            )
    });
    h_flex()
        .gap_0p5()
        .when(!editor.history.is_empty(), |s| {
            s.child(div().pr_1().child("hist"))
        })
        .children(dots.collect::<Vec<_>>())
}

fn headers_table(resp: &HttpResponse, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .id("resp-headers-table")
        .size_full()
        .overflow_y_scroll()
        .px_4()
        .py_2()
        .text_sm()
        .children(resp.headers.iter().map(|h| {
            h_flex()
                .gap_4()
                .py_0p5()
                .items_start()
                .child(
                    div()
                        .w(px(220.))
                        .flex_none()
                        .text_color(theme.muted_foreground)
                        .child(h.name.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .text_color(theme.foreground)
                        .child(h.value.clone()),
                )
        }))
        .into_any_element()
}

fn empty(editor: &RequestEditor, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (title, hint) = if editor.sending {
        (
            t!("sending… %{elapsed}", elapsed = editor.elapsed()).into_owned(),
            t!("⌘↵ cancels"),
        )
    } else {
        (
            t!("no response yet").into_owned(),
            t!("⌘↵ sends the request"),
        )
    };
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_1()
        .text_sm()
        // Intrinsic size 48 in sleep.svg: gpui rasterizes SVGs at ×2 of it.
        .when(!editor.sending, |s| {
            s.child(img("sleep.svg").size(px(48.)).mb_2())
        })
        .child(div().text_color(theme.foreground).child(title))
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(hint),
        )
        .into_any_element()
}
