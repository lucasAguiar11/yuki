//! Render of the "Configurações" screen: sections on the left, the selected
//! one's fields on the right.

use gpui_kit::component::{
    button::Button,
    h_flex,
    input::{Input, Textarea},
    v_flex, ActiveTheme as _, Sizable as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{
    prefs::{Font, Prefs},
    settings::{Section, SettingsScreen},
    ui, CloseSettings, FocusNext, FocusPrev,
};

impl Render for SettingsScreen {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mono = cx.theme().mono_font_family.clone();
        h_flex()
            .size_full()
            .key_context("SettingsScreen")
            .track_focus(&self.focus)
            .on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &FocusPrev, window, cx| window.focus_prev(cx))
            .font_family(mono)
            .text_sm()
            .child(self.render_list(cx))
            .child(self.render_section(cx))
    }
}

/// `label  control` with an optional hint under the control.
fn field(
    label: impl Into<SharedString>,
    control: impl IntoElement,
    hint: Option<String>,
    cx: &App,
) -> Div {
    let (muted, faint) = (cx.theme().muted_foreground, ui::faint(cx));
    h_flex()
        .items_start()
        .gap_3()
        .child(
            div()
                .w(px(150.))
                .whitespace_nowrap()
                .flex_none()
                .pt_1()
                .text_xs()
                .text_color(muted)
                .child(Into::<SharedString>::into(label)),
        )
        .child(
            v_flex()
                .flex_1()
                .max_w(px(560.))
                .gap_1()
                .child(control)
                .children(hint.map(|h| div().text_xs().text_color(faint).child(h))),
        )
}

impl SettingsScreen {
    fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (fg, muted, active_bg, hover_bg, border, sidebar) = (
            theme.foreground,
            theme.muted_foreground,
            theme.list_active,
            theme.list_hover,
            theme.border,
            theme.sidebar,
        );
        let rows = [Section::Workspace, Section::Appearance].map(|section| {
            let selected = section == self.section;
            div()
                .id(section.id())
                .h(px(28.))
                .px_3()
                .flex()
                .items_center()
                .cursor_pointer()
                .text_color(if selected { fg } else { muted })
                .when(selected, |s| s.bg(active_bg))
                .hover(move |s| s.bg(hover_bg))
                .child(section.label())
                .on_click(cx.listener(move |this, _, _, cx| this.select(section, cx)))
        });
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
                    .child(t!("settings")),
            )
            .children(rows)
    }

    fn render_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.section {
            Section::Workspace => self.render_workspace(cx),
            Section::Appearance => self.render_appearance(cx),
        };
        let theme = cx.theme();
        let (fg, muted, border, red) = (
            theme.foreground,
            theme.muted_foreground,
            theme.border,
            theme.red,
        );
        let title = self.section.label();
        let header = h_flex()
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
                    .id("settings-close")
                    .text_xs()
                    .cursor_pointer()
                    .text_color(muted)
                    .hover(move |s| s.text_color(fg))
                    .child(t!("close esc"))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(CloseSettings), cx)),
            );
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(header)
            .when(!self.error.is_empty(), |s| {
                s.child(
                    div()
                        .px_4()
                        .pt_2()
                        .text_xs()
                        .text_color(red)
                        .child(self.error.clone()),
                )
            })
            .child(
                v_flex()
                    .id("settings-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_4()
                    .py_3()
                    .gap_4()
                    .child(body),
            )
    }

    fn render_workspace(&self, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .gap_4()
            .child(field(t!("name").into_owned(), Input::new(&self.name).small(), None, cx))
            .child(field(
                "dns",
                Textarea::new(&self.dns).small(),
                Some(t!("one host per line: 127.0.0.1 api.local · # disables. applies to this workspace only and goes to git.").into_owned()),
                cx,
            ))
            .child(field(
                t!("shell env").into_owned(),
                Input::new(&self.env).small(),
                Some(t!("variables from your shell that requests can use as {{ NAME }}, below environments. this machine only.").into_owned()),
                cx,
            ))
            .into_any_element()
    }

    fn render_appearance(&self, cx: &mut Context<Self>) -> AnyElement {
        let stepper = |id: &'static str, font: Font, cx: &mut Context<Self>| {
            let size = Prefs::size(font, cx);
            let step = |sign: &'static str, delta: f32, cx: &mut Context<Self>| {
                Button::new(SharedString::from(format!("{id}-{sign}")))
                    .xsmall()
                    .outline()
                    .label(sign)
                    .on_click(cx.listener(move |this, _, _, cx| this.bump(font, delta, cx)))
            };
            h_flex()
                .gap_2()
                .child(step("−", -1., cx))
                .child(div().w(px(36.)).text_center().child(format!("{size}")))
                .child(step("+", 1., cx))
        };
        let ui_font = stepper("font-ui", Font::Ui, cx);
        let code_font = stepper("font-code", Font::Code, cx);
        v_flex()
            .gap_4()
            .child(field(t!("interface font").into_owned(), ui_font, None, cx))
            .child(field(t!("code font").into_owned(), code_font, None, cx))
            .into_any_element()
    }
}

impl Section {
    fn id(self) -> &'static str {
        match self {
            Section::Workspace => "section-workspace",
            Section::Appearance => "section-appearance",
        }
    }

    fn label(self) -> SharedString {
        match self {
            Section::Workspace => t!("workspace"),
            Section::Appearance => t!("appearance"),
        }
        .into_owned()
        .into()
    }
}
