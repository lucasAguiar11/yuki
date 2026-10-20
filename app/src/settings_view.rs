//! Render of the "Configurações" screen: sections on the left, the selected
//! one's fields on the right.

use gpui_kit::component::{
    button::Button,
    checkbox::Checkbox,
    h_flex,
    input::{Input, Textarea},
    select::Select,
    v_flex, ActiveTheme as _, Sizable as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{
    prefs::{FamilySelect, Font, Prefs},
    settings::{Section, SettingsScreen},
    ui, CloseSettings, ExportWorkspace, FocusNext, FocusPrev,
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
pub(crate) fn field(
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
                .min_w_0()
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
        let rows = [
            Section::General,
            Section::Workspace,
            Section::Appearance,
            Section::Mcp,
            Section::About,
        ]
        .map(|section| {
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
            Section::General => self.render_general(cx),
            Section::Workspace => self.render_workspace(cx),
            Section::Appearance => self.render_appearance(cx),
            Section::Mcp => self.render_mcp(cx),
            Section::About => self.render_about(cx),
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
        let (fg, blue) = (cx.theme().foreground, cx.theme().blue);
        let folder = self.folder.clone();
        let path = div()
            .id("ws-folder")
            .cursor_pointer()
            .text_color(blue)
            .hover(move |s| s.text_color(fg))
            .child(self.folder.display().to_string())
            .on_click(move |_, _, cx| cx.reveal_path(&folder));
        let path = h_flex().gap_3().child(path).child(
            Button::new("ws-move")
                .xsmall()
                .outline()
                .label(t!("move…").into_owned())
                .on_click(cx.listener(|this, _, window, cx| this.move_folder(window, cx))),
        );
        v_flex()
            .gap_4()
            .child(field(t!("name").into_owned(), Input::new(&self.name).small(), None, cx))
            .child(field(
                t!("folder").into_owned(),
                path,
                Some(t!("where its files and git repo live · click to show in Finder · move takes everything, .git included").into_owned()),
                cx,
            ))
            .child(field(
                t!("export").into_owned(),
                h_flex().child(
                    Button::new("ws-export")
                        .xsmall()
                        .outline()
                        .label(t!("export…").into_owned())
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(ExportWorkspace), cx)
                        }),
                ),
                Some(t!("a file for Postman, Insomnia, Yaak or Voleeo").into_owned()),
                cx,
            ))
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
            .child(field(
                t!("certificates").into_owned(),
                Checkbox::new("skip-certs")
                    .checked(self.skip_certs)
                    .label(t!("accept invalid certificates").into_owned())
                    .on_click(cx.listener(|this, on: &bool, _, cx| this.set_skip_certs(*on, cx))),
                Some(t!("expired, self-signed or for another host: sends go through anyway. this machine only.").into_owned()),
                cx,
            ))
            .into_any_element()
    }

    fn render_general(&self, cx: &mut Context<Self>) -> AnyElement {
        let (fg, blue) = (cx.theme().foreground, cx.theme().blue);
        let base = cx.global::<Prefs>().workspaces_dir.clone();
        let base_text = base.as_ref().map_or_else(
            || t!("each one where it is").into_owned(),
            |b| b.display().to_string(),
        );
        let base_row = h_flex()
            .gap_3()
            .child(
                div()
                    .id("ws-base")
                    .when_some(base, |s, base| {
                        s.cursor_pointer()
                            .text_color(blue)
                            .hover(move |s| s.text_color(fg))
                            .on_click(move |_, _, cx| cx.reveal_path(&base))
                    })
                    .child(base_text),
            )
            .child(
                Button::new("ws-move-all")
                    .xsmall()
                    .outline()
                    .label(t!("move…").into_owned())
                    .on_click(cx.listener(|this, _, window, cx| this.move_all(window, cx))),
            );
        v_flex()
            .gap_4()
            .child(field(
                t!("workspaces folder").into_owned(),
                base_row,
                Some(t!("one folder each, named after the workspace · move takes them all · new imports land here too").into_owned()),
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
        let family =
            |select: &Entity<FamilySelect>| div().w(px(240.)).child(Select::new(select).small());
        let ui_font = h_flex()
            .gap_4()
            .child(stepper("font-ui", Font::Ui, cx))
            .child(family(&self.ui_family));
        let code_font = h_flex()
            .gap_4()
            .child(stepper("font-code", Font::Code, cx))
            .child(family(&self.code_family));
        // Language names stay in their own language: readable whatever is active.
        let current = cx.global::<Prefs>().language.clone();
        let theme = cx.theme();
        let (fg, muted, active_bg, hover_bg) = (
            theme.foreground,
            theme.muted_foreground,
            theme.list_active,
            theme.list_hover,
        );
        let langs = [
            (None, t!("system").into_owned()),
            (Some("en"), "english".into()),
            (Some("pt-BR"), "português".into()),
        ]
        .map(|(lang, label): (Option<&'static str>, String)| {
            let selected = current.as_deref() == lang;
            div()
                .id(SharedString::from(format!(
                    "lang-{}",
                    lang.unwrap_or("system")
                )))
                .px_2()
                .py_0p5()
                .rounded_sm()
                .cursor_pointer()
                .text_color(if selected { fg } else { muted })
                .when(selected, |s| s.bg(active_bg))
                .hover(move |s| s.bg(hover_bg))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| this.set_language(lang, cx)))
        });
        let choice = |id: &'static str, label: String, selected: bool| {
            div()
                .id(id)
                .px_2()
                .py_0p5()
                .rounded_sm()
                .cursor_pointer()
                .text_color(if selected { fg } else { muted })
                .when(selected, |s| s.bg(active_bg))
                .hover(move |s| s.bg(hover_bg))
                .child(label)
        };
        // Response below (stacked) or beside the request editor; ⌘⌥L flips it.
        let beside = cx.global::<Prefs>().response_beside;
        let layouts = [(false, t!("below")), (true, t!("beside"))].map(|(on, label)| {
            let id = if on { "layout-beside" } else { "layout-below" };
            choice(id, label.into_owned(), beside == on).on_click(cx.listener(
                move |this, _, _, cx| {
                    let prefs = Prefs::set_response_beside(on, cx);
                    this.save_prefs(prefs, cx);
                },
            ))
        });
        v_flex()
            .gap_4()
            .child(field(t!("interface font").into_owned(), ui_font, None, cx))
            .child(field(t!("code font").into_owned(), code_font, None, cx))
            .child(field(
                "",
                h_flex().child(
                    div()
                        .id("fonts-reset")
                        .text_xs()
                        .cursor_pointer()
                        .text_color(muted)
                        .hover(move |s| s.text_color(fg))
                        .child(t!("restore default fonts").into_owned())
                        .on_click(cx.listener(|this, _, window, cx| this.reset_fonts(window, cx))),
                ),
                None,
                cx,
            ))
            .child(field(
                t!("language").into_owned(),
                h_flex().gap_2().children(langs),
                Some(t!("open tabs finish switching after a restart.").into_owned()),
                cx,
            ))
            .child(field(
                t!("response").into_owned(),
                h_flex().gap_2().children(layouts),
                Some(t!("⌘⌥L switches it from anywhere.").into_owned()),
                cx,
            ))
            .into_any_element()
    }
}

impl Section {
    fn id(self) -> &'static str {
        match self {
            Section::General => "section-general",
            Section::Workspace => "section-workspace",
            Section::Appearance => "section-appearance",
            Section::Mcp => "section-mcp",
            Section::About => "section-about",
        }
    }

    fn label(self) -> SharedString {
        match self {
            Section::General => t!("general"),
            Section::Workspace => t!("workspace"),
            Section::Appearance => t!("appearance"),
            Section::Mcp => t!("agents (mcp)"),
            Section::About => t!("about"),
        }
        .into_owned()
        .into()
    }
}
