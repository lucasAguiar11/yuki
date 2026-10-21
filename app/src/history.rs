//! `history 16 ▾` at the end of the status strip: the request's past sends as
//! a list (status, time, size, when); picking one reopens that response.

use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    ActiveTheme as _, Sizable as _,
};
use gpui_kit::*;

use crate::{editor::RequestEditor, ui};

/// `08:39:14` today, `06/10 14:16` on another day.
fn when(iso: &str) -> String {
    let Ok(t) = chrono::DateTime::parse_from_rfc3339(iso) else {
        return ui::clock(iso);
    };
    let t = t.with_timezone(&chrono::Local);
    if t.date_naive() == chrono::Local::now().date_naive() {
        t.format("%H:%M:%S").to_string()
    } else {
        t.format("%d/%m %H:%M").to_string()
    }
}

pub fn render(editor: &RequestEditor, cx: &mut Context<RequestEditor>) -> Option<AnyElement> {
    if editor.history.is_empty() {
        return None;
    }
    let shown = match &editor.response {
        Some(Ok(r)) => r.response_id.clone(),
        _ => String::new(),
    };
    let rows: Vec<_> = editor
        .history
        .iter()
        .map(|s| {
            (
                s.id.clone(),
                ui::status_color(s.status, cx),
                format!("{} {}", s.status, s.status_text),
                format!("{:.0} ms", s.total_ms),
                ui::human_bytes(s.body_size),
                when(&s.recorded_at),
            )
        })
        .collect();
    let (muted, mono) = (
        cx.theme().muted_foreground,
        cx.theme().mono_font_family.clone(),
    );
    let this = cx.entity().downgrade();
    Some(
        Button::new("history")
            .ghost()
            .xsmall()
            .text_color(muted)
            .label(t!("history %{n} ▾", n = editor.history.len()).into_owned())
            .dropdown_menu(move |mut m, _, _| {
                m = m.min_w(px(340.)).max_h(px(360.)).scrollable(true);
                for (id, color, status, ms, size, at) in rows.iter().cloned() {
                    let (this, mono) = (this.clone(), mono.clone());
                    let checked = id == shown;
                    m = m.item(
                        PopupMenuItem::element(move |_, _| {
                            // Fixed columns so times and sizes line up row to row.
                            h_flex()
                                .w_full()
                                .gap_3()
                                .text_xs()
                                .font_family(mono.clone())
                                .child(
                                    div()
                                        .w(px(110.))
                                        .truncate()
                                        .text_color(color)
                                        .child(status.clone()),
                                )
                                .child(div().w(px(64.)).text_color(muted).child(ms.clone()))
                                .child(div().w(px(60.)).text_color(muted).child(size.clone()))
                                .child(div().flex_1().child(at.clone()))
                        })
                        .checked(checked)
                        .on_click(move |_, window, cx| {
                            let id = id.clone();
                            _ = this.update(cx, |e, cx| e.show_history(id, window, cx));
                        }),
                    );
                }
                m
            })
            .into_any_element(),
    )
}
