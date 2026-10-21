use gpui_kit::base::text::TextView;
use gpui_kit::component::{
    h_flex, input::Editor, tooltip::Tooltip, v_flex, ActiveTheme as _, Icon, Sizable as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_core::HttpResponse;

use crate::{
    editor::{RequestEditor, RespTab},
    sent, ui, CaptureField, CopyForLlm,
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

fn is_csv(resp: &HttpResponse) -> bool {
    resp.body_is_text && crate::csv::is_csv(crate::binary::content_type(resp))
}

/// HTML renders as a page, CSV as a table.
fn has_preview(resp: &HttpResponse) -> bool {
    resp.body_is_text && crate::binary::content_type(resp) == "text/html" || is_csv(resp)
}

/// The tab actually shown: preview falls back to body when there's nothing to preview.
fn shown_tab(editor: &RequestEditor, resp: &HttpResponse) -> RespTab {
    match editor.resp_tab {
        RespTab::Preview if !has_preview(resp) => RespTab::Body,
        t => t,
    }
}

pub fn render(editor: &RequestEditor, cx: &mut Context<RequestEditor>) -> AnyElement {
    if !editor.response_open {
        // Under the editor: the strip alone, status and history still visible.
        return div()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(strip(editor, cx))
            .into_any_element();
    }
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
                RespTab::Body if !resp.body_is_text && resp.sse_frames.is_empty() => {
                    crate::binary::render(editor, resp, cx)
                }
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
                RespTab::Preview if is_csv(resp) => crate::csv::render(&resp.body, cx),
                RespTab::Preview => TextView::html("resp-html", resp.body.clone())
                    .scrollable(true)
                    .selectable(true)
                    .size_full()
                    .px_4()
                    .into_any_element(),
                RespTab::Headers => headers_table(resp, cx),
                RespTab::Timeline => sent::render(
                    resp,
                    editor.sent.as_ref(),
                    editor.reveal_tokens,
                    &editor.timeline_focus,
                    cx,
                ),
            },
        };
    v_flex()
        .size_full()
        // Nothing sent yet: the sleeping cat says it; an empty strip would too.
        .when(
            editor.response.is_some() || editor.sending || !editor.history.is_empty(),
            |r| r.child(strip(editor, cx)),
        )
        .child(div().flex_1().min_h_0().child(content))
        // Under the body: appearing on a click shrinks the editor from the
        // bottom, so the clicked text stays put.
        .when(
            match &editor.response {
                Some(Ok(r)) => shown_tab(editor, r),
                _ => editor.resp_tab,
            } == RespTab::Body,
            |r| r.children(editor.render_capture_bar(cx)),
        )
        .into_any_element()
}

/// `200 OK · 128 ms · 1.0 KB · 14:03:12   body  headers 20  timeline   ●●●●`
fn strip(editor: &RequestEditor, cx: &mut Context<RequestEditor>) -> impl IntoElement {
    let toggle = cx.listener(|this, _, _, cx| this.toggle_response(cx));
    let theme = cx.theme();
    let (fg, muted) = (theme.foreground, theme.muted_foreground);
    let mut row = h_flex()
        .h(px(30.))
        .px_4()
        .gap_2()
        .text_xs()
        .font_family(theme.mono_font_family.clone())
        .text_color(muted)
        .bg(theme.sidebar)
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .id("toggle-response")
                .cursor_pointer()
                .hover(move |s| s.text_color(fg))
                .child(if editor.response_open { "▾" } else { "▸" })
                .tooltip(|window, cx| {
                    Tooltip::new(t!("response ⌘J").into_owned()).build(window, cx)
                })
                .on_click(toggle),
        );

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
                    this.response_open = true;
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
            .when(resp.events.iter().any(|e| e.kind == "tls"), |r| {
                r.child(
                    div()
                        .id("resp-tls")
                        .text_color(cx.theme().warning)
                        .child(t!("⚠ unverified"))
                        .tooltip(|window, cx| {
                            Tooltip::new(t!("the TLS certificate wasn't checked: settings → workspace → accept invalid certificates is on").into_owned())
                                .build(window, cx)
                        }),
                )
            })
            .children(at.map(|t| div().text_color(ui::faint(cx)).child(t)))
            .child(div().w(px(16.)))
            .child(tab("resp-body", "body".into(), RespTab::Body, cx))
            .when(has_preview(resp), |r| {
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
                let text = sent::copy_text(resp, editor.sent.as_ref(), editor.reveal_tokens);
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
                .child(
                    div()
                        .id("timeline-reveal")
                        .px_1p5()
                        .cursor_pointer()
                        .hover(move |s| s.text_color(fg))
                        .child(if editor.reveal_tokens {
                            t!("hide tokens")
                        } else {
                            t!("show tokens")
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.reveal_tokens = !this.reveal_tokens;
                            cx.notify();
                        })),
                )
            });
    } else if editor.sending {
        row = row.child(t!("sending… %{elapsed}", elapsed = editor.elapsed()).into_owned());
    }
    // Any finished response (SSE has its frames, not a body): its file, as sent.
    let download =
        matches!(&editor.response, Some(Ok(r)) if r.sse_frames.is_empty()) && !editor.sending;
    let (fg, hover_bg, radius) = (
        cx.theme().foreground,
        cx.theme().list_hover,
        cx.theme().radius,
    );
    row.child(div().flex_1())
        .children(
            editor
                .file_note
                .clone()
                .map(|n| div().max_w(px(360.)).truncate().child(n)),
        )
        .when(download, |r| {
            // Icon-only actions on the response, tooltip names them.
            let icon = |id: &'static str, icon: assets::IconName, tip: &'static str| {
                div()
                    .id(id)
                    .p_0p5()
                    .rounded(radius)
                    .cursor_pointer()
                    .hover(move |s| s.text_color(fg).bg(hover_bg))
                    .child(Icon::new(icon).xsmall())
                    .tooltip(move |window, cx| Tooltip::new(t!(tip).into_owned()).build(window, cx))
            };
            r.child(
                icon("resp-llm", assets::IconName::Bot, "copy for LLM")
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(CopyForLlm), cx)),
            )
            .child(
                icon("resp-download", assets::IconName::Download, "download…")
                    .on_click(cx.listener(|this, _, _, cx| this.download(cx))),
            )
        })
        .children(crate::history::render(editor, cx))
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
