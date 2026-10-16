//! An older send picked in the history (●●●): the request pane shows the
//! request as it was then, read-only, with `restore` (bring it back into the
//! editor, saved like any edit) and `back` (the current request).

use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{draft, editor::RequestEditor, ui};

impl RequestEditor {
    /// `POST ▸ url · sent at 10:48:41      restore · back`, in place of the prompt.
    pub(crate) fn render_past_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (at, req) = self.past.as_ref()?;
        let theme = cx.theme();
        let (fg, muted, hover) = (
            theme.foreground,
            theme.muted_foreground,
            theme.secondary_hover,
        );
        let action = |id: &'static str, label: String| {
            div()
                .id(id)
                .px_2()
                .py_0p5()
                .rounded(theme.radius)
                .cursor_pointer()
                .text_sm()
                .text_color(fg)
                .hover(move |s| s.bg(hover))
                .child(label)
        };
        Some(
            h_flex()
                .h(px(52.))
                .px_4()
                .gap_3()
                .font_family(theme.mono_font_family.clone())
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(ui::method_color(&req.method, cx))
                        .child(req.method.clone()),
                )
                .child(div().text_color(muted).child("▸"))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(fg)
                        .child(req.url.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(t!("as sent at %{at}", at = at).into_owned()),
                )
                .child(
                    action("past-restore", t!("restore").into_owned())
                        .on_click(cx.listener(|this, _, window, cx| this.restore_past(window, cx))),
                )
                .child(
                    action("past-back", t!("back").into_owned()).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.past = None;
                            cx.notify();
                        },
                    )),
                )
                .into_any_element(),
        )
    }

    /// Params, headers, body and auth of the past request, read-only.
    pub(crate) fn render_past(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (_, req) = self.past.as_ref()?;
        let theme = cx.theme();
        let (fg, muted) = (theme.foreground, theme.muted_foreground);
        let heading = |s: String| div().pt_2().text_color(muted).child(s);
        let pairs = |rows: &[yuki_core::RequestParameter]| {
            rows.iter()
                .filter(|p| !p.name.is_empty())
                .map(|p| {
                    h_flex()
                        .gap_2()
                        .when(!p.enabled, |d| d.opacity(0.45))
                        .child(div().text_color(muted).child(format!("{}:", p.name)))
                        .child(div().text_color(fg).child(p.value.clone()))
                })
                .collect::<Vec<_>>()
        };
        let body = req
            .body
            .as_ref()
            .filter(|b| b.kind != yuki_core::BodyKind::None);
        Some(
            v_flex()
                .id("past-request")
                .size_full()
                .overflow_y_scroll()
                .px_4()
                .py_2()
                .gap_0p5()
                .font_family(theme.mono_font_family.clone())
                .text_xs()
                .when(!req.parameters.is_empty(), |s| {
                    s.child(heading("params".into()))
                        .children(pairs(&req.parameters))
                })
                .child(heading(format!("headers {}", req.headers.len())))
                .children(pairs(&req.headers))
                .child(heading(format!(
                    "body {}",
                    body.map_or("—".into(), |b| ui::kind_name(&b.kind))
                )))
                .when_some(body, |s, b| {
                    let fields = b.fields.as_deref().unwrap_or_default();
                    s.children(b.text.lines().map(|l| {
                        div().text_color(fg).child(if l.is_empty() {
                            " ".to_string()
                        } else {
                            l.to_string()
                        })
                    }))
                    .children(fields.iter().map(|f| {
                        h_flex()
                            .gap_2()
                            .child(div().text_color(muted).child(format!("{}:", f.name)))
                            .child(div().text_color(fg).child(f.value.clone()))
                    }))
                })
                // The scheme only: its secrets stay out of a read-only view.
                .child(heading(format!("auth {}", ui::kind_name(&req.auth))))
                .into_any_element(),
        )
    }

    /// The past request becomes the current one (into the active variation,
    /// if any), saved and rebuilt like any change.
    fn restore_past(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, past)) = self.past.take() else {
            return;
        };
        self.change(|req| *req = draft::fold(req, past), window, cx);
    }
}
