//! Response "log" tab: a load run's sends (`HttpResponse::load_log`), one
//! line each; ▸ opens a send's request (`>` lines, body as sent) and
//! response (`<` lines, body). Credentials masked like the timeline.

use std::collections::HashSet;

use gpui_kit::base::SelectableText;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_core::{HttpRequest, LoadLogEntry};

use crate::{editor::RequestEditor, sent};

fn has_detail(e: &LoadLogEntry) -> bool {
    !e.events.is_empty() || e.body.is_some()
}

/// `   18 ms ✓ #3 200 OK · 18 ms`
fn line(e: &LoadLogEntry) -> String {
    let (mark, _) = sent::mark(&e.kind);
    format!("{:>8} {mark} {}", sent::ms(e.at_ms), e.text)
}

/// The whole log, every send's request and response included, for "copy all".
pub fn copy_text(log: &[LoadLogEntry], sent_req: Option<&HttpRequest>, reveal: bool) -> String {
    let req_body = sent_req.and_then(sent::body_text);
    let mut out = Vec::new();
    for e in log {
        out.push(line(e));
        if !has_detail(e) {
            continue;
        }
        out.extend(
            sent::lines(&e.events, None, reveal)
                .into_iter()
                .map(|(l, _)| format!("    {l}")),
        );
        if let Some(b) = &req_body {
            out.push(format!("    request body:\n{b}"));
        }
        if let Some(b) = &e.body {
            out.push(format!("    response body:\n{b}"));
        }
    }
    out.join("\n")
}

impl RequestEditor {
    pub(crate) fn toggle_log_entry(&mut self, ix: usize, cx: &mut Context<Self>) {
        if !self.log_open.remove(&ix) {
            self.log_open.insert(ix);
        }
        cx.notify();
    }
}

pub fn render(
    log: &[LoadLogEntry],
    open: &HashSet<usize>,
    sent_req: Option<&HttpRequest>,
    reveal: bool,
    focus: &FocusHandle,
    cx: &mut Context<RequestEditor>,
) -> AnyElement {
    let theme = cx.theme();
    let (fg, muted, red, panel) = (
        theme.foreground,
        theme.muted_foreground,
        theme.red,
        theme.sidebar,
    );
    let (radius, mono) = (theme.radius, theme.mono_font_family.clone());
    let req_body = sent_req.and_then(sent::body_text);
    // One counter across the view: a drag (and ⌘C) reads top to bottom.
    let mut order = 0u64;
    let mut run = |text: String| {
        order += 1;
        SelectableText::new(("load-log", order as usize), text).document_order(order)
    };
    let boxed = |label: String, text: SelectableText| {
        v_flex()
            .gap_0p5()
            .child(div().text_color(muted).child(label))
            .child(
                div()
                    .p_2()
                    .rounded(radius)
                    .bg(panel)
                    .text_color(fg)
                    .child(text),
            )
    };
    let mut rows: Vec<AnyElement> = Vec::with_capacity(log.len());
    for (ix, e) in log.iter().enumerate() {
        let is_open = open.contains(&ix);
        let color = match e.kind.as_str() {
            "error" => red,
            "info" => muted,
            _ => fg,
        };
        // Outside the text: a click inside a SelectableText starts a selection.
        let toggle = div()
            .id(("load-log-toggle", ix))
            .w(px(14.))
            .flex_none()
            .when(has_detail(e), |d| {
                d.cursor_pointer()
                    .text_color(muted)
                    .hover(|s| s.text_color(fg))
                    .child(if is_open { "▾" } else { "▸" })
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_log_entry(ix, cx)))
            });
        rows.push(
            h_flex()
                .items_start()
                .child(toggle)
                .child(div().text_color(color).child(run(line(e))))
                .into_any_element(),
        );
        if !is_open {
            continue;
        }
        let mut detail = v_flex().pl(px(14.)).pt_1().pb_2().gap_1();
        for (text, wire) in sent::lines(&e.events, None, reveal) {
            detail = detail.child(
                div()
                    .text_color(if wire { fg } else { muted })
                    .child(run(text)),
            );
        }
        if let Some(b) = &req_body {
            detail = detail.child(boxed(t!("request body").into_owned(), run(b.clone())));
        }
        if let Some(b) = &e.body {
            detail = detail.child(boxed(t!("response body").into_owned(), run(b.clone())));
        }
        rows.push(detail.into_any_element());
    }
    let focus_on_click = focus.clone();
    v_flex()
        .id("load-log")
        // As the timeline: focused on click so ⌘C reaches the selection.
        .track_focus(focus)
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            focus_on_click.focus(window, cx)
        })
        .on_mouse_move(|e, window, _| {
            if e.dragging() {
                window.refresh()
            }
        })
        .size_full()
        .overflow_y_scroll()
        .px_4()
        .py_3()
        .gap_0p5()
        .text_xs()
        .font_family(mono)
        .children(rows)
        .into_any_element()
}
