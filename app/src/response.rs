use gpui_kit::component::{h_flex, input::Editor, tooltip::Tooltip, v_flex, ActiveTheme as _};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_core::HttpResponse;

use crate::{
    editor::{RequestEditor, RespTab},
    ui,
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
            format!("<binário, {}>", ui::human_bytes(resp.body_size)),
            "text",
        );
    }
    match serde_json::from_str::<serde_json::Value>(&resp.body) {
        Ok(v) => (
            serde_json::to_string_pretty(&v).unwrap_or_else(|_| resp.body.clone()),
            "json",
        ),
        Err(_) => (resp.body.clone(), "text"),
    }
}

pub fn render(editor: &RequestEditor, cx: &mut Context<RequestEditor>) -> AnyElement {
    let theme = cx.theme();
    let content = match &editor.response {
        None => empty(editor.sending, cx),
        Some(Err(e)) => v_flex()
            .p_4()
            .gap_2()
            .text_sm()
            .child(div().text_color(theme.red).child("✕ falha no envio"))
            .child(div().text_color(theme.foreground).child(e.clone()))
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("confira a URL e o ambiente selecionado"),
            )
            .into_any_element(),
        Some(Ok(resp)) => match editor.resp_tab {
            RespTab::Body => div()
                .size_full()
                .pl_2()
                .child(
                    Editor::new(&editor.response_body)
                        .appearance(false)
                        .readonly(true)
                        .h_full(),
                )
                .into_any_element(),
            RespTab::Headers => headers_table(resp, cx),
        },
    };
    v_flex()
        .size_full()
        .child(strip(editor, cx))
        .child(div().flex_1().min_h_0().child(content))
        .into_any_element()
}

/// `200 OK · 128 ms · 1.0 KB      body  headers 20      ●●●●`
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

    if let Some(Ok(resp)) = &editor.response {
        let tab = |id: &'static str, label: String, t: RespTab, cx: &mut Context<RequestEditor>| {
            let active = editor.resp_tab == t;
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
        row = row
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(ui::status_color(resp.status, cx))
                    .child(format!("{} {}", resp.status, resp.status_text)),
            )
            .child(format!("{:.0} ms", resp.timing.total_ms))
            .child(ui::human_bytes(resp.body_size))
            .child(div().w(px(16.)))
            .child(tab("resp-body", "body".into(), RespTab::Body, cx))
            .child(tab(
                "resp-headers",
                format!("headers {}", resp.headers.len()),
                RespTab::Headers,
                cx,
            ));
    } else {
        row = row.child(if editor.sending {
            "enviando…"
        } else {
            "sem resposta"
        });
    }
    row.child(div().flex_1()).child(history(editor, cx))
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

fn empty(sending: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (title, hint) = if sending {
        ("enviando…", "⌘↵ cancela")
    } else {
        ("nenhuma resposta ainda", "⌘↵ envia o request")
    };
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_1()
        .text_sm()
        .child(div().text_color(theme.foreground).child(title))
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(hint),
        )
        .into_any_element()
}
