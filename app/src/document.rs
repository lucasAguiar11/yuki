//! The request as a document: prompt line, meta line, optional editor, then
//! the response (see `response.rs`).

use gpui_kit::component::{
    h_flex,
    input::{InlineTokenContext, Input},
    resizable_panel,
    select::Select,
    spinner::Spinner,
    v_flex, v_resizable, ActiveTheme as _, Sizable as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{
    editor::{ReqTab, RequestEditor},
    response,
    table::Which,
    ui, var_view, CompleteAccept, CompleteDismiss, CompleteNext, CompleteOpen, CompletePrev, Send,
    ToggleEditor, ToggleVars,
};

impl Render for RequestEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let response = response::render(self, cx);
        let body = if self.editor_open {
            v_resizable(SharedString::from(format!("doc-{}", self.saved.id)))
                .child(
                    resizable_panel()
                        .size(px(240.))
                        .size_range(px(80.)..Pixels::MAX)
                        .flex_none()
                        .child(self.render_editor(cx)),
                )
                .child(resizable_panel().child(response))
                .into_any_element()
        } else {
            response
        };
        let popup = self.render_completion(window, cx);
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            // Wins over the input's own ↑↓↵⇥esc while the popup shows.
            .when(popup.is_some(), |d| d.key_context("VarComplete"))
            .on_action(
                cx.listener(|this, _: &CompleteNext, window, cx| {
                    this.move_completion(1, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &CompletePrev, window, cx| {
                this.move_completion(-1, window, cx)
            }))
            .on_action(cx.listener(|this, _: &CompleteAccept, window, cx| {
                this.accept_completion(None, window, cx)
            }))
            .on_action(cx.listener(|this, _: &CompleteDismiss, _, cx| this.dismiss_completion(cx)))
            .on_action(
                cx.listener(|this, _: &CompleteOpen, window, cx| this.open_completion(window, cx)),
            )
            .child(self.render_prompt(window, cx))
            .child(self.render_meta(cx))
            .child(div().flex_1().min_h_0().child(body))
            .children(popup)
    }
}

impl RequestEditor {
    /// `GET ▸ https://…   ⌘↵ send` — the caret brightens while the URL has focus.
    fn render_prompt(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let focused = self.url.read(cx).focus_handle(cx).is_focused(window);
        let action = if self.sending {
            h_flex()
                .gap_2()
                .child(Spinner::new().small().color(theme.muted_foreground))
                .child(t!("cancel"))
        } else {
            h_flex()
                .gap_2()
                .child(div().text_color(theme.muted_foreground).child("⌘↵"))
                .child(t!("send"))
        };
        h_flex()
            .h(px(52.))
            .px_4()
            .gap_2()
            .border_b_1()
            .border_color(if focused { theme.ring } else { theme.border })
            .child(
                div()
                    .w(px(92.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .font_family(theme.mono_font_family.clone())
                    .text_color(ui::method_color(&self.saved.method, cx))
                    .child(Select::new(&self.method).appearance(false)),
            )
            .child(
                div()
                    .text_color(if focused {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .child("▸"),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(14.))
                    .font_family(theme.mono_font_family.clone())
                    .child(
                        Input::new(&self.url)
                            .appearance(false)
                            .token(self.chip_renderer()),
                    ),
            )
            .child(
                div()
                    .id("send")
                    .px_2()
                    .py_1()
                    .rounded(theme.radius)
                    .text_sm()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.secondary_hover))
                    .child(action)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(Send), cx)),
            )
    }

    /// `headers 2 · body json · auth bearer          ▸ request ⌘E`
    fn render_meta(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let body_kind = self
            .draft(cx)
            .body
            .map_or("—".into(), |b| ui::kind_name(&b.kind));
        let segment = |id: &'static str, label: String, tab: ReqTab, cx: &mut Context<Self>| {
            let active = self.editor_open && self.req_tab == tab;
            div()
                .id(id)
                .px_1p5()
                .rounded(cx.theme().radius)
                .cursor_pointer()
                .text_color(if active {
                    cx.theme().foreground
                } else {
                    cx.theme().muted_foreground
                })
                .when(active, |s| s.bg(cx.theme().secondary))
                .hover(|s| s.text_color(cx.theme().foreground))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_editor(Some(tab), cx)))
        };
        // Only when the URL has `:name`s or a query — otherwise it's noise.
        let count = self.params_count(cx);
        let params = (count > 0)
            .then(|| segment("meta-params", format!("params {count}"), ReqTab::Params, cx));
        let headers = segment(
            "meta-headers",
            format!("headers {}", self.header_count(cx)),
            ReqTab::Headers,
            cx,
        );
        let body = segment("meta-body", format!("body {body_kind}"), ReqTab::Body, cx);
        let auth = segment(
            "meta-auth",
            format!("auth {}", self.auth.label()),
            ReqTab::Auth,
            cx,
        );
        let notes = segment("meta-notes", self.notes_label(cx).into(), ReqTab::Notes, cx);
        let vars_segment = self.render_vars_segment(cx);
        let theme = cx.theme();
        let dot = || div().text_color(theme.muted_foreground).child("·");
        h_flex()
            .h(px(30.))
            .px_4()
            .gap_1()
            .text_xs()
            .font_family(theme.mono_font_family.clone())
            .text_color(theme.muted_foreground)
            .border_b_1()
            .border_color(theme.border)
            .when_some(params, |s, p| s.child(p).child(dot()))
            .child(headers)
            .child(dot())
            .child(body)
            .child(dot())
            .child(auth)
            .child(dot())
            .child(notes)
            .child(dot())
            .child(vars_segment)
            .child(div().flex_1())
            .child(
                div()
                    .id("toggle-editor")
                    .cursor_pointer()
                    .hover(|s| s.text_color(theme.foreground))
                    .child(if self.editor_open {
                        "▾ request ⌘E"
                    } else {
                        "▸ request ⌘E"
                    })
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleEditor), cx)),
            )
    }

    fn render_editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let pane = match self.req_tab {
            ReqTab::Params => self.render_params(cx),
            ReqTab::Headers => self.render_table(Which::Headers, false, cx),
            ReqTab::Body => self.render_body(cx),
            ReqTab::Auth => self.render_auth(cx),
            ReqTab::Notes => self.render_notes(cx),
        };
        div()
            .size_full()
            .px_4()
            .py_2()
            .font_family(cx.theme().mono_font_family.clone())
            .child(pane)
            .into_any_element()
    }

    pub(crate) fn chip_renderer(
        &self,
    ) -> impl Fn(&InlineTokenContext, &mut Window, &mut App) -> AnyElement + 'static {
        let vars = self.vars.clone();
        let params = self.params.entries();
        move |ctx, _, cx| var_view::chip(ctx, &vars, &params, cx).into_any_element()
    }

    /// `vars 4` — or `vars 4 · 1 missing` in red; opens the variables panel.
    fn render_vars_segment(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let used = self.used_vars(cx).len();
        let missing = self.missing_vars(cx).len();
        h_flex()
            .id("meta-vars")
            .px_1p5()
            .gap_1()
            .rounded(theme.radius)
            .cursor_pointer()
            .hover(|s| s.text_color(theme.foreground))
            .child(format!("vars {used}"))
            .when(missing > 0, |s| {
                s.child(
                    div()
                        .text_color(theme.red)
                        .child(t!("· %{n} missing", n = missing).into_owned()),
                )
            })
            .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleVars), cx))
    }
}
