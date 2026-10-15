//! The request as a document: prompt line, meta line, optional editor, then
//! the response (see `response.rs`).

use gpui_kit::component::{
    h_flex, h_resizable,
    input::{InlineTokenContext, Input, InputState},
    resizable_panel,
    select::Select,
    spinner::Spinner,
    tooltip::Tooltip,
    v_flex, v_resizable, ActiveTheme as _, Icon, IconName, ResizableState, Sizable as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{
    auth_form::{AuthForm, AuthHost},
    chip_templates,
    complete::{CompleteHost, Completion, Target},
    complete_view,
    editor::{ReqTab, RequestEditor},
    response,
    session::Session,
    table::Which,
    ui, var_view, Send, ToggleEditor, ToggleLayout, ToggleVars,
};

impl CompleteHost for RequestEditor {
    fn completion(&self) -> &Option<Completion> {
        &self.complete
    }
    fn completion_mut(&mut self) -> &mut Option<Completion> {
        &mut self.complete
    }
    fn template_vars(&self) -> &crate::vars::Vars {
        &self.vars
    }
    fn workspace(&self) -> (std::sync::Arc<crate::backend::Backend>, String) {
        (self.backend.clone(), self.saved.workspace_id.clone())
    }
    fn template_fields(&self) -> Vec<Target> {
        let mut targets: Vec<Target> = vec![(&self.url).into()];
        targets.extend(self.params.inputs().map(Target::Line));
        targets.extend(self.query.inputs().map(Target::Line));
        targets.extend(self.headers.inputs().map(Target::Line));
        targets.extend(self.body.fields.inputs().map(Target::Line));
        targets.extend(self.body.code_editors().map(Target::Code));
        targets.extend(self.auth.inputs().map(Target::Line));
        targets
    }
}

impl AuthHost for RequestEditor {
    fn form(&self) -> &AuthForm {
        &self.auth
    }
    fn form_mut(&mut self) -> &mut AuthForm {
        &mut self.auth
    }
    fn stored_auth(&self) -> &yuki_core::AuthConfig {
        &self.shown.auth
    }
    fn auth_edited(&mut self, cx: &mut Context<Self>) {
        self.refresh_dirty(cx);
    }
    fn auth_typed(
        &mut self,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        chip_templates!(input, window, cx);
        self.update_completion(input, cx);
    }
    fn decorate(&self, input: Input) -> Input {
        input.token(self.chip_renderer())
    }
    fn can_fetch_token(&self) -> bool {
        true
    }
    fn fetch_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        RequestEditor::fetch_token(self, window, cx)
    }
}

impl Render for RequestEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let response = response::render(self, cx);
        let beside = cx.global::<crate::prefs::Prefs>().response_beside;
        // One resizable group per layout: a group reused across directions
        // would keep the other one's sizes.
        let id = |dir: &str| SharedString::from(format!("doc-{dir}-{}", self.saved.id));
        let s = cx.global::<Session>();
        let (height, width) = (px(s.editor_height), px(s.editor_width));
        // The editor is the first panel; its size is what a new group starts at.
        let first = |state: &Entity<ResizableState>, cx: &App| {
            state.read(cx).sizes().first().map(|p| p.as_f32())
        };
        let body = match (self.editor_open, beside) {
            (false, _) => response,
            (true, false) => v_resizable(id("v"))
                .on_resize(move |state, _, cx| {
                    if let Some(h) = first(state, cx) {
                        cx.global_mut::<Session>().editor_height = h;
                    }
                })
                .child(
                    resizable_panel()
                        .size(height)
                        .size_range(px(80.)..Pixels::MAX)
                        .flex_none()
                        .child(self.render_editor(cx)),
                )
                .child(resizable_panel().child(response))
                .into_any_element(),
            (true, true) => h_resizable(id("h"))
                .on_resize(move |state, _, cx| {
                    if let Some(w) = first(state, cx) {
                        cx.global_mut::<Session>().editor_width = w;
                    }
                })
                .child(
                    resizable_panel()
                        .size(width)
                        .size_range(px(280.)..Pixels::MAX)
                        .flex_none()
                        .child(self.render_editor(cx)),
                )
                .child(resizable_panel().child(response))
                .into_any_element(),
        };
        let popup = complete_view::render(self, window, cx);
        let root = v_flex().size_full().bg(cx.theme().background);
        complete_view::keys(root, popup.is_some(), cx)
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
            .edited(cx)
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
        let variation = self.render_variation(cx);
        let beside = cx.global::<crate::prefs::Prefs>().response_beside;
        let theme = cx.theme();
        let (fg, muted, selected_bg) = (theme.foreground, theme.muted_foreground, theme.secondary);
        // Response below / beside: the current one lit; the other one switches.
        let layout = |id: &'static str, icon: IconName, on: bool, tip: String| {
            let current = on == beside;
            div()
                .id(id)
                .p_0p5()
                .rounded(theme.radius)
                .cursor_pointer()
                .text_color(if current { fg } else { muted })
                .when(current, |d| d.bg(selected_bg))
                .hover(move |s| s.text_color(fg))
                .child(Icon::new(icon).xsmall())
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .on_click(move |_, window, cx| {
                    if !current {
                        window.dispatch_action(Box::new(ToggleLayout), cx)
                    }
                })
        };
        let layouts = h_flex()
            .gap_0p5()
            .mr_3()
            .child(layout(
                "layout-below",
                IconName::PanelBottom,
                false,
                t!("response below · ⌘⌥L").into_owned(),
            ))
            .child(layout(
                "layout-beside",
                IconName::PanelRight,
                true,
                t!("response beside · ⌘⌥L").into_owned(),
            ));
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
            .child(variation)
            .child(dot())
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
            .child(layouts)
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
            ReqTab::Auth => crate::auth_form::render(self, cx),
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
