//! How `{{ VAR }}` templates look and explain themselves: inline chips in the
//! URL/headers, with a tooltip card showing the resolved value and its origin.

use std::rc::Rc;

use gpui_kit::component::{
    h_flex, input::InlineTokenContext, tooltip::Tooltip, v_flex, ActiveTheme as _,
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
                cx.notify();
            })
        })
        .detach();
    }

    /// Templates referenced by the request as currently edited.
    pub fn used_vars(&self, cx: &App) -> Vec<String> {
        let (url, headers, body) = (
            self.url.read(cx).value(),
            self.headers.read(cx).value(),
            self.body.read(cx).value(),
        );
        vars::used([url.as_ref(), headers.as_ref(), body.as_ref()])
    }

    pub fn missing_vars(&self, cx: &App) -> Vec<String> {
        let mut used = self.used_vars(cx);
        used.retain(|e| vars::kind(e, &self.vars) == Kind::Missing);
        used
    }
}

fn kind_color(kind: Kind, cx: &App) -> Hsla {
    let t = cx.theme();
    match kind {
        Kind::Defined => t.blue,
        Kind::Missing => t.red,
        Kind::Function => t.magenta,
    }
}

/// Inline chip: the expression without braces, tinted by state; hovering shows
/// the resolved value and its origin.
pub fn chip(ctx: &InlineTokenContext, vars: &Vars, cx: &App) -> impl IntoElement {
    let expr = ctx.token().id().clone();
    let kind = vars::kind(&expr, vars);
    let var = vars.get(expr.as_ref()).cloned();
    let color = kind_color(kind, cx);
    div()
        .id(SharedString::from(format!(
            "chip-{}-{}",
            expr,
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
        .border_color(color.opacity(if kind == Kind::Missing { 0.7 } else { 0.25 }))
        .text_color(color)
        .child(
            div()
                .min_w_0()
                .text_ellipsis()
                .child(ctx.token().label().clone()),
        )
        // A tooltip, not app state: re-rendering the input on hover would reset
        // its token hover tracking.
        .tooltip(move |window, cx| {
            let (expr, var) = (expr.clone(), var.clone());
            Tooltip::element(move |_, cx| card(&expr, kind, var.as_ref(), cx)).build(window, cx)
        })
}

/// Name, origin, resolved value (secrets masked), or why there is none.
fn card(expr: &str, kind: Kind, var: Option<&Var>, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let muted = |text: String| div().text_color(theme.muted_foreground).child(text);
    let origin = match (kind, var) {
        (Kind::Function, _) => "função".to_string(),
        (_, Some(v)) => v.source.label(),
        (_, None) => "não definida".to_string(),
    };
    let mut lines = v_flex().gap_1();
    match (kind, var) {
        (Kind::Function, _) => lines = lines.child(muted("avaliada no momento do envio".into())),
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
        (_, None) => {
            lines = lines
                .child(
                    div()
                        .text_color(theme.red)
                        .child("sem valor no ambiente atual"),
                )
                .child(muted(
                    "defina no ambiente, no global ou numa pasta · ⌘⇧E lista tudo".into(),
                ))
        }
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
