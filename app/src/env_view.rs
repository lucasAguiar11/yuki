//! Render of the "Environments" screen: environments on the left, the selected
//! one's variables as a name/value table on the right.

use gpui_kit::component::{
    button::ButtonVariant, checkbox::Checkbox, dialog::AlertDialog, h_flex, input::Input, v_flex,
    ActiveTheme as _, Sizable as _, WindowExt as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{
    env_screen::{EnvScreen, Naming},
    ui, CloseEnvs, FocusNext, FocusPrev,
};

impl Render for EnvScreen {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let mono = theme.mono_font_family.clone();
        h_flex()
            .size_full()
            .key_context("EnvScreen")
            .track_focus(&self.focus)
            .on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &FocusPrev, window, cx| window.focus_prev(cx))
            .font_family(mono)
            .text_sm()
            .child(self.render_list(cx))
            .child(self.render_vars(cx))
    }
}

impl EnvScreen {
    fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (fg, muted, faint, active_bg, hover_bg, border, sidebar) = (
            theme.foreground,
            theme.muted_foreground,
            ui::faint(cx),
            theme.list_active,
            theme.list_hover,
            theme.border,
            theme.sidebar,
        );
        let rows = self.envs.iter().enumerate().map(|(ix, env)| {
            let selected = ix == self.selected;
            let n = env.variables.len();
            h_flex()
                .id(("env", ix))
                .h(px(28.))
                .px_3()
                .gap_2()
                .cursor_pointer()
                .text_color(if selected { fg } else { muted })
                .when(selected, |s| s.bg(active_bg))
                .hover(move |s| s.bg(hover_bg))
                // Slot always reserved so names line up with or without a color.
                .child(dot(ui::env_color(&env.color), 8.))
                .child(div().flex_1().truncate().child(if ix == 0 {
                    "global".to_string()
                } else {
                    env.name.clone()
                }))
                .child(div().text_xs().text_color(faint).child(n.to_string()))
                .on_click(cx.listener(move |this, _, window, cx| this.select(ix, window, cx)))
        });
        let new_env = match &self.naming {
            Some((Naming::New, input)) => div()
                .px_2()
                .pt_1()
                .child(Input::new(input).small())
                .into_any_element(),
            _ => div()
                .id("env-new")
                .h(px(28.))
                .px_3()
                .flex()
                .items_center()
                .cursor_pointer()
                .text_color(muted)
                .hover(move |s| s.text_color(fg))
                .child(t!("+ new environment"))
                .on_click(
                    cx.listener(|this, _, window, cx| this.start_naming(Naming::New, window, cx)),
                )
                .into_any_element(),
        };
        v_flex()
            .w(px(220.))
            .flex_none()
            .h_full()
            .bg(sidebar)
            .border_r_1()
            .border_color(border)
            .child(
                div()
                    .h(px(36.))
                    .px_3()
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(muted)
                    .child(t!("environments")),
            )
            .children(rows.collect::<Vec<_>>())
            .child(new_env)
    }

    fn render_vars(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // Built first: the click listeners need `cx` mutably, `theme` borrows it.
        let swatches = (!self.is_global()).then(|| self.swatches(cx));
        let theme = cx.theme();
        let (fg, muted, faint, border, red, yellow) = (
            theme.foreground,
            theme.muted_foreground,
            ui::faint(cx),
            theme.border,
            theme.red,
            theme.yellow,
        );
        let env = self.current();
        let global = self.is_global();
        let title: AnyElement = match &self.naming {
            Some((Naming::Rename, input)) => div()
                .w(px(260.))
                .child(Input::new(input).small())
                .into_any_element(),
            _ => div()
                .text_color(fg)
                .font_weight(FontWeight::SEMIBOLD)
                .child(if global {
                    "global".into()
                } else {
                    env.name.clone()
                })
                .into_any_element(),
        };
        let action = |id: &'static str, label: std::borrow::Cow<'static, str>| {
            div()
                .id(id)
                .text_xs()
                .cursor_pointer()
                .text_color(muted)
                .hover(move |s| s.text_color(fg))
                .child(label)
        };
        let shared = env.shared;
        let header = h_flex()
            .h(px(36.))
            .px_4()
            .gap_3()
            .border_b_1()
            .border_color(border)
            .child(title)
            .children(swatches)
            .child(
                div()
                    .id("env-shared")
                    .text_xs()
                    .px_1p5()
                    .rounded(theme.radius)
                    .cursor_pointer()
                    .text_color(if shared { yellow } else { muted })
                    .border_1()
                    .border_color(border)
                    .child(if shared {
                        t!("in git")
                    } else {
                        t!("this machine only")
                    })
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(if shared {
                            t!("saved in the workspace folder: goes into commits. click to keep it on this machine only").into_owned()
                        } else {
                            t!("saved outside the repository: never goes to git. click to version it").into_owned()
                        })
                        .build(window, cx)
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_shared(cx))),
            )
            .child(div().flex_1())
            .when(!global, |h| {
                h.child(action("env-rename", t!("rename")).on_click(cx.listener(
                    |this, _, window, cx| this.start_naming(Naming::Rename, window, cx),
                )))
                .child(action("env-delete", t!("delete")).on_click(cx.listener(
                    |this, _, window, cx| this.confirm_delete(window, cx),
                )))
            })
            .child(
                action("env-close", t!("close esc"))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(CloseEnvs), cx)),
            );

        let hint = if global {
            t!("applies to every request, in any environment; the active environment overrides it.")
        } else {
            t!("active via the env picker in the title bar; overrides the globals.")
        };
        let leak = shared && self.rows.iter().any(|r| r.secret) && !self.encrypted_ws;
        let head = h_flex()
            .h(px(26.))
            .gap_2()
            .px_4()
            .text_xs()
            .text_color(faint)
            .border_b_1()
            .border_color(border)
            .child(div().w(px(16.)))
            .child(div().w(relative(0.3)).px_2p5().child(t!("name")))
            .child(div().flex_1().px_2p5().child(t!("value")))
            .child(div().w(px(64.)).child(t!("secret")))
            .child(div().w(px(16.)));
        let last = self.rows.len().saturating_sub(1);
        let rows = self.rows.iter().enumerate().map(|(ix, row)| {
            let blank = ix == last;
            let check = Checkbox::new(("var-on", ix))
                .checked(row.enabled)
                .tab_stop(false)
                .xsmall()
                .on_click(
                    cx.listener(move |this, on: &bool, _, cx| this.toggle_enabled(ix, *on, cx)),
                );
            let secret = div()
                .id(("var-secret", ix))
                .w(px(64.))
                .text_xs()
                .when(!blank, |d| {
                    d.cursor_pointer()
                        .text_color(if row.secret { yellow } else { faint })
                        .hover(move |s| s.text_color(fg))
                        .child(if row.secret { t!("● yes") } else { t!("○ no") })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.toggle_secret(ix, window, cx)
                        }))
                });
            let remove = div()
                .id(("var-rm", ix))
                .w(px(16.))
                .cursor_pointer()
                .text_color(faint)
                .opacity(0.)
                .when(!blank, |d| {
                    d.group_hover("var-row", |s| s.opacity(1.))
                        .hover(move |s| s.text_color(red))
                        .child("×")
                        .on_click(cx.listener(move |this, _, _, cx| this.remove(ix, cx)))
                });
            let mut value = Input::new(&row.value).appearance(false);
            if row.secret {
                value = value.mask_toggle();
            }
            h_flex()
                .group("var-row")
                .h(px(30.))
                .px_4()
                .gap_2()
                .border_b_1()
                .border_color(border.opacity(0.5))
                .child(div().w(px(16.)).when(!blank, |d| d.child(check)))
                .child(
                    div()
                        .w(relative(0.3))
                        .text_color(theme.blue)
                        .when(!row.enabled, |d| d.opacity(0.45))
                        .child(Input::new(&row.key).appearance(false)),
                )
                .child(
                    div()
                        .flex_1()
                        .text_color(fg)
                        .when(!row.enabled, |d| d.opacity(0.45))
                        .child(value),
                )
                .child(secret)
                .child(remove)
        });
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(header)
            .child(
                div()
                    .px_4()
                    .py_2()
                    .text_xs()
                    .text_color(muted)
                    .child(hint),
            )
            .when(leak, |s| {
                s.child(div().px_4().pb_2().text_xs().text_color(yellow).child(
                    t!("warning: this workspace isn't encrypted — secrets of an environment in git are stored in plain text."),
                ))
            })
            .when(!self.error.is_empty(), |s| {
                s.child(
                    div()
                        .px_4()
                        .pb_2()
                        .text_xs()
                        .text_color(red)
                        .child(self.error.clone()),
                )
            })
            .child(head)
            .child(
                div()
                    .id("env-vars")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows.collect::<Vec<_>>()),
            )
    }

    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let env = self.current();
        let title: SharedString = t!("delete the environment “%{name}”?", name = env.name)
            .into_owned()
            .into();
        let n = env.variables.len();
        let detail: SharedString = match n {
            0 => t!("the environment is empty.").into_owned().into(),
            1 => t!("its variable will be deleted from disk.").into_owned().into(),
            n => t!("its %{n} variables will be deleted from disk.", n = n)
                .into_owned()
                .into(),
        };
        let this = cx.entity().downgrade();
        let build = move |alert: AlertDialog, _: &mut Window, _: &mut App| {
            let this = this.clone();
            alert
                .title(title.clone())
                .description(detail.clone())
                .show_cancel(true)
                .ok_text(t!("delete").into_owned())
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(t!("cancel").into_owned())
                .on_ok(move |_, window, cx| {
                    _ = this.update(cx, |s, cx| s.delete(window, cx));
                    true
                })
        };
        // Deferred: the click that triggered this would pull focus back out.
        let focus = self.focus.clone();
        window.defer(cx, move |window, cx| {
            focus.focus(window, cx);
            window.open_alert_dialog(cx, build);
        });
    }

    /// `○ ● ● ● ● ● ●` — no color, then the theme's; the current one ringed.
    fn swatches(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.current().color.to_lowercase();
        let (ring, muted) = (cx.theme().foreground, cx.theme().muted_foreground);
        let chip = |id: usize, color: String, fill: Option<Hsla>, cx: &mut Context<Self>| {
            let on = color == current || (fill.is_none() && ui::env_color(&current).is_none());
            div()
                .id(("env-color", id))
                .size(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .border_1()
                .border_color(if on { ring } else { transparent_black() })
                .child(match fill {
                    Some(c) => dot(Some(c), 10.).into_any_element(),
                    // "No color": an empty ring.
                    None => div()
                        .size(px(10.))
                        .rounded_full()
                        .border_1()
                        .border_color(muted)
                        .into_any_element(),
                })
                .on_click(cx.listener(move |this, _, _, cx| this.set_color(color.clone(), cx)))
        };
        let mut row = h_flex().gap_0p5().child(chip(0, String::new(), None, cx));
        for (ix, hex) in ui::env_palette(cx).into_iter().enumerate() {
            let fill = ui::env_color(&hex);
            row = row.child(chip(ix + 1, hex, fill, cx));
        }
        row
    }
}

/// A filled circle, or an empty slot of the same size.
pub fn dot(color: Option<Hsla>, size: f32) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .when_some(color, |d, c| d.bg(c))
}
