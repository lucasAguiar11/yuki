//! How `{{ VAR }}` templates look and explain themselves: inline chips in the
//! URL/headers, with a tooltip card showing the resolved value and its origin.

use std::rc::Rc;

use gpui_kit::component::{
    h_flex,
    input::{InlineTokenContext, InputState},
    tooltip::Tooltip,
    v_flex, ActiveTheme as _,
};
use gpui_kit::*;

use crate::{
    editor::RequestEditor,
    vars::{self, Kind, Var, Vars},
};

/// Turn any freshly typed `{{ … }}` in an input/textarea entity into a chip.
/// A macro because `InputState` and `TextareaState` share the token API
/// without sharing a trait.
#[macro_export]
macro_rules! chip_templates {
    ($entity:expr, $window:expr, $cx:expr) => {
        $entity.update($cx, |state, cx| {
            let text = state.value();
            let chipped: Vec<_> = state.tokens().iter().map(|s| s.range()).collect();
            // Back to front so earlier ranges stay valid while replacing.
            for (range, expr) in $crate::vars::spans(&text).into_iter().rev() {
                if !chipped.contains(&range) {
                    let chip = $crate::vars::token(&text[range.clone()], expr);
                    let _ = state.replace_range_with_token(range, chip, $window, cx);
                }
            }
        })
    };
}

/// Chip the URL's finished `:name` path params (`vars::path_spans`).
pub fn chip_path_params(url: &Entity<InputState>, window: &mut Window, cx: &mut App) {
    url.update(cx, |state, cx| {
        let (text, cursor) = (state.value(), state.cursor());
        let chipped: Vec<_> = state.tokens().iter().map(|s| s.range()).collect();
        let mut changed = false;
        for range in vars::path_spans(&text, Some(cursor)).into_iter().rev() {
            if !chipped.contains(&range) {
                let chip = vars::path_token(&text[range.clone()]);
                changed |= state
                    .replace_range_with_token(range, chip, window, cx)
                    .is_ok();
            }
        }
        // Chipping parks the caret at the chip's end; the `/` typed after
        // it would land before it. The text is unchanged, so put it back.
        if changed {
            state.set_selected_range(cursor..cursor, cx);
        }
    })
}

pub const MASK: &str = "••••••••";

impl RequestEditor {
    pub fn set_env(&mut self, env_id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.env_id = env_id;
        self.load_vars(window, cx);
    }

    pub fn load_vars(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let (ws, folder, env) = (
            self.saved.workspace_id.clone(),
            self.saved.folder_id.clone(),
            self.env_id.clone(),
        );
        let task = cx.background_spawn(async move {
            backend.variables(&ws, folder.as_deref(), env.as_deref())
        });
        cx.spawn_in(window, async move |this, cx| {
            let vars = task.await;
            this.update(cx, |this, cx| {
                this.vars = Rc::new(vars);
                this.body.paint(&this.vars, cx);
                cx.notify();
            })
        })
        .detach();
    }

    /// Templates referenced by the request as currently edited.
    pub fn used_vars(&self, cx: &App) -> Vec<String> {
        let mut texts = vec![self.url.read(cx).value()];
        texts.extend(self.params.texts(cx));
        texts.extend(self.headers.texts(cx));
        texts.extend(self.body.template_texts(cx));
        texts.extend(self.auth.texts(cx));
        vars::used(texts.iter().map(|s| s.as_ref()))
    }

    pub fn missing_vars(&self, cx: &App) -> Vec<String> {
        let mut used = self.used_vars(cx);
        used.retain(|e| vars::kind(e, &self.vars) == Kind::Missing);
        used
    }
}

pub(crate) fn kind_color(kind: Kind, cx: &App) -> Hsla {
    let t = cx.theme();
    match kind {
        Kind::Defined => t.blue,
        Kind::Missing => t.red,
        Kind::Function => t.magenta,
    }
}

/// Inline chip: the expression without braces, tinted by state; hovering shows
/// the resolved value and its origin.
pub fn chip(
    ctx: &InlineTokenContext,
    vars: &Vars,
    params: &[(String, Entity<InputState>)],
    cx: &App,
) -> impl IntoElement {
    let expr = ctx.token().id().clone();
    if let Some(name) = expr.strip_prefix(':') {
        return path_chip(ctx, name, params, cx).into_any_element();
    }
    let kind = vars::kind(&expr, vars);
    let var = vars.get(expr.as_ref()).cloned();
    let color = kind_color(kind, cx);
    shell(ctx, color, kind == Kind::Missing, cx)
        // A tooltip, not app state: re-rendering the input on hover would reset
        // its token hover tracking.
        .tooltip(move |window, cx| {
            let (expr, var) = (expr.clone(), var.clone());
            Tooltip::element(move |_, cx| card(&expr, kind, var.as_ref(), cx)).build(window, cx)
        })
        .into_any_element()
}

/// `:name` chip; the tooltip shows the value it gets from the params tab.
fn path_chip(
    ctx: &InlineTokenContext,
    name: &str,
    params: &[(String, Entity<InputState>)],
    cx: &App,
) -> impl IntoElement {
    let value = params
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.clone());
    let empty = value.as_ref().is_none_or(|v| v.read(cx).value().is_empty());
    let color = cx.theme().blue;
    let label = ctx.token().label().clone();
    shell(ctx, color, empty, cx).tooltip(move |window, cx| {
        let (label, value) = (label.clone(), value.clone());
        Tooltip::element(move |_, cx| {
            let theme = cx.theme();
            let shown = value
                .as_ref()
                .map(|v| v.read(cx).value())
                .unwrap_or_default();
            let line = if shown.is_empty() {
                div()
                    .text_color(theme.red)
                    .child(t!("no value — fill it in params"))
            } else {
                div().text_color(theme.foreground).child(shown)
            };
            v_flex()
                .max_w(px(360.))
                .gap_1p5()
                .text_xs()
                .font_family(theme.mono_font_family.clone())
                .child(
                    h_flex()
                        .justify_between()
                        .gap_4()
                        .child(div().text_color(theme.blue).child(label.clone()))
                        .child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(t!("path param")),
                        ),
                )
                .child(line)
        })
        .build(window, cx)
    })
}

/// Tinted pill around the token's label; `strong` = emphasized border.
fn shell(ctx: &InlineTokenContext, color: Hsla, strong: bool, cx: &App) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!(
            "chip-{}-{}",
            ctx.token().id(),
            ctx.range().start
        )))
        .flex()
        .items_center()
        .h(ctx.line_height())
        .max_w(ctx.available_width())
        .px_1()
        .rounded(cx.theme().radius)
        .bg(color.opacity(if ctx.is_selected() { 0.32 } else { 0.13 }))
        .border_1()
        .border_color(color.opacity(if strong { 0.7 } else { 0.25 }))
        .text_color(color)
        .child(
            div()
                .min_w_0()
                .text_ellipsis()
                .child(ctx.token().label().clone()),
        )
}

/// Name, origin, resolved value (secrets masked), or why there is none.
fn card(expr: &str, kind: Kind, var: Option<&Var>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let muted = |text: String| div().text_color(theme.muted_foreground).child(text);
    let origin = match (kind, var) {
        (Kind::Function, _) => t!("function").into_owned(),
        (_, Some(v)) => v.source.label(),
        (_, None) => t!("not defined").into_owned(),
    };
    let mut lines = v_flex().gap_1();
    match (kind, var) {
        (Kind::Function, _) => {
            lines = lines.child(muted(t!("evaluated at send time").into_owned()))
        }
        (_, Some(v)) => {
            let shown = if v.secret {
                MASK.to_string()
            } else {
                v.value.clone()
            };
            lines = lines.child(div().text_color(theme.foreground).child(shown));
            if !v.secret && v.raw != v.value {
                lines = lines.child(muted(format!("= {}", v.raw)));
            }
        }
        (_, None) => lines = lines
            .child(
                div()
                    .text_color(theme.red)
                    .child(t!("no value in the current environment")),
            )
            .child(muted(
                t!("define it in the environment, in global or in a folder · ⌘⇧E lists everything")
                    .into_owned(),
            )),
    }
    v_flex()
        .max_w(px(360.))
        .gap_1p5()
        .text_xs()
        .font_family(theme.mono_font_family.clone())
        .child(
            h_flex()
                .justify_between()
                .gap_4()
                .child(
                    div()
                        .text_color(kind_color(kind, cx))
                        .child(expr.to_string()),
                )
                .child(muted(origin)),
        )
        .child(lines)
}
