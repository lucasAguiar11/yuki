//! Render of the import screen: header, destination, tree of checkboxes.

use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::Input,
    select::Select,
    v_flex, ActiveTheme as _, Disableable as _, Sizable as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_import::ImportNode;

use crate::{
    import_view::{format_label, requests, ImportScreen},
    settings_view::field,
    transfer::TransferEvent,
    ui, CloseTransfer,
};

impl ImportScreen {
    fn rows(
        &self,
        nodes: &[ImportNode],
        depth: usize,
        out: &mut Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) {
        let faint = ui::faint(cx);
        for n in nodes {
            let node = n.clone();
            let on = self.all_checked(n);
            let label = h_flex()
                .gap_2()
                .when_some(n.method.as_deref(), |s, m| {
                    s.child(
                        div()
                            .w(px(52.))
                            .text_color(ui::method_color(m, cx))
                            .child(m.to_uppercase()),
                    )
                })
                .child(n.name.clone())
                .when_some(n.path.as_deref().filter(|p| !p.is_empty()), |s, p| {
                    s.child(div().text_color(faint).truncate().child(p.to_string()))
                });
            out.push(
                h_flex()
                    .pl(px(12. + depth as f32 * 16.))
                    .h(px(26.))
                    .gap_2()
                    .child(
                        Checkbox::new(SharedString::from(format!("imp-{}", n.id)))
                            .checked(on)
                            .on_click(cx.listener(move |this, on: &bool, _, cx| {
                                this.toggle(&node, *on, cx)
                            })),
                    )
                    .child(label)
                    .into_any_element(),
            );
            self.rows(&n.children, depth + 1, out, cx);
        }
    }
}

impl Render for ImportScreen {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows = vec![];
        self.rows(&self.preview.tree.clone(), 0, &mut rows, cx);
        let mut total = vec![];
        requests(&self.preview.tree, &mut total);
        let (fg, muted, red, border, active_bg, hover_bg) = {
            let t = cx.theme();
            (
                t.foreground,
                t.muted_foreground,
                t.red,
                t.border,
                t.list_active,
                t.list_hover,
            )
        };
        let chip = |id: &'static str,
                    label: String,
                    on: bool,
                    to_current: bool,
                    cx: &mut Context<Self>| {
            div()
                .id(id)
                .px_2()
                .py_0p5()
                .rounded_sm()
                .cursor_pointer()
                .text_color(if on { fg } else { muted })
                .when(on, |s| s.bg(active_bg))
                .hover(move |s| s.bg(hover_bg))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.to_current = to_current;
                    cx.notify();
                }))
        };
        let mut dest = h_flex().gap_2().child(chip(
            "dest-new",
            t!("new workspace").into_owned(),
            !self.to_current,
            false,
            cx,
        ));
        if let Some(ws) = &self.current {
            let label = t!("into %{name}", name = ws.name).into_owned();
            dest = dest.child(chip("dest-current", label, self.to_current, true, cx));
        }
        let target = if self.to_current {
            div()
                .w(px(320.))
                .child(Select::new(&self.folder).small())
                .into_any_element()
        } else {
            div()
                .w(px(320.))
                .child(Input::new(&self.name).small())
                .into_any_element()
        };
        let all = total.clone();
        let select_all = h_flex()
            .gap_3()
            .text_xs()
            .child(
                t!(
                    "%{n} of %{total} requests",
                    n = self.checked.len(),
                    total = total.len()
                )
                .into_owned(),
            )
            .child(
                div()
                    .id("imp-all")
                    .cursor_pointer()
                    .text_color(muted)
                    .hover(move |s| s.text_color(fg))
                    .child(t!("all").into_owned())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.checked = all.iter().cloned().collect();
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("imp-none")
                    .cursor_pointer()
                    .text_color(muted)
                    .hover(move |s| s.text_color(fg))
                    .child(t!("none").into_owned())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.checked.clear();
                        cx.notify();
                    })),
            );
        let warnings = self
            .preview
            .warnings
            .iter()
            .map(|w| div().text_xs().text_color(muted).child(format!("· {w}")));
        let title = format!(
            "{} · {}{}",
            t!("import"),
            format_label(self.format),
            self.preview
                .format_version
                .as_deref()
                .map(|v| format!(" {v}"))
                .unwrap_or_default()
        );
        v_flex()
            .size_full()
            .key_context("Transfer")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &CloseTransfer, _, cx| cx.emit(TransferEvent::Close)))
            .font_family(cx.theme().mono_font_family.clone())
            .text_sm()
            .child(
                h_flex()
                    .h(px(36.))
                    .px_4()
                    .border_b_1()
                    .border_color(border)
                    .child(
                        div()
                            .text_color(fg)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("imp-close")
                            .text_xs()
                            .cursor_pointer()
                            .text_color(muted)
                            .hover(move |s| s.text_color(fg))
                            .child(t!("close esc").into_owned())
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(CloseTransfer), cx)
                            }),
                    ),
            )
            .child(
                v_flex()
                    .px_4()
                    .py_3()
                    .gap_4()
                    .child(field(
                        t!("destination").into_owned(),
                        v_flex().gap_2().child(dest).child(target),
                        None,
                        cx,
                    ))
                    .when(!self.preview.warnings.is_empty(), |s| {
                        s.child(field(
                            t!("warnings").into_owned(),
                            v_flex().children(warnings),
                            None,
                            cx,
                        ))
                    })
                    .child(field(t!("items").into_owned(), select_all, None, cx)),
            )
            .child(
                v_flex()
                    .id("imp-tree")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .border_t_1()
                    .border_b_1()
                    .border_color(border)
                    .py_1()
                    .children(rows),
            )
            .child(
                h_flex()
                    .px_4()
                    .py_2()
                    .gap_3()
                    .child(
                        div()
                            .text_xs()
                            .text_color(red)
                            .flex_1()
                            .child(self.error.clone()),
                    )
                    .child(
                        Button::new("imp-go")
                            .small()
                            .primary()
                            .label(
                                if self.busy {
                                    t!("importing…")
                                } else {
                                    t!("import")
                                }
                                .into_owned(),
                            )
                            .disabled(!self.can_import())
                            .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
                    ),
            )
    }
}
