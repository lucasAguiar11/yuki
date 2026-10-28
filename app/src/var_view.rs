//! How `{{ VAR }}` templates look and explain themselves: inline chips in the
//! URL/headers and in the code editors (body), with a tooltip card showing
//! the resolved value and its origin.

use std::rc::Rc;

use gpui_kit::component::{
    h_flex,
    input::{EditorState, InlineTokenClickEvent, InlineTokenContext, Input, InputState},
    tooltip::Tooltip,
    v_flex, ActiveTheme as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_mcp::resolve::response_args;

use crate::{
    editor::RequestEditor,
    ui,
    vars::{self, Kind, Origin, Refs, Vars},
};

/// ⌘-click on a chip: open where it's defined (`vars::origin`).
#[derive(Clone, PartialEq, Action)]
#[action(namespace = yuki, no_json)]
pub struct GoToOrigin(pub Origin);

type Params = Vec<(String, Entity<InputState>)>;

/// Draws each chip; `⌘` held turns the ones that lead somewhere into links.
fn renderer(
    vars: Rc<Vars>,
    refs: Rc<Refs>,
    params: Params,
) -> impl Fn(&InlineTokenContext, &mut Window, &mut App) -> AnyElement + 'static {
    move |ctx, window, cx| {
        let cmd = window.modifiers().platform;
        chip(ctx, &vars, &refs, &params, cmd, cx).into_any_element()
    }
}

/// ⌘-click goes to the origin.
fn on_click(vars: Rc<Vars>) -> impl Fn(&InlineTokenClickEvent, &mut Window, &mut App) + 'static {
    move |ev, window, cx| {
        if !ev.click().modifiers().platform {
            return;
        }
        if let Some(origin) = vars::origin(ev.token().id(), &vars) {
            window.dispatch_action(Box::new(GoToOrigin(origin)), cx);
        }
    }
}

/// Template/path-param chips on `input`.
pub fn chips(input: Input, vars: Rc<Vars>, refs: Rc<Refs>, params: Params) -> Input {
    let click = on_click(vars.clone());
    input
        .token(renderer(vars, refs, params))
        .on_token_click(click)
}

/// The same chips in a code editor. Its `Editor` control has no token
/// builder, so they're pinned on the state (patched gpui-base); called on
/// every render so they read the current variables.
pub fn pin_chips(editor: &Entity<EditorState>, vars: Rc<Vars>, refs: Rc<Refs>, cx: &mut App) {
    let click = Rc::new(on_click(vars.clone()));
    let draw = Rc::new(renderer(vars, refs, vec![]));
    editor.update(cx, |s, _| s.pin_token_presentation(draw, Some(click)));
}

/// Turn any freshly typed `{{ … }}` in an input/textarea/code-editor entity
/// into a chip. A macro because the three states share the token API without
/// sharing a trait.
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
    pub fn set_env(&mut self, env_id: Option<String>, cx: &mut Context<Self>) {
        self.env_id = env_id;
        self.load_vars(cx);
    }

    pub fn load_vars(&mut self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let (ws, folder, env) = (
            self.saved.workspace_id.clone(),
            self.saved.folder_id.clone(),
            self.env_id.clone(),
        );
        let mut calls = self.used_vars(cx);
        calls.retain(|e| response_args(e).is_some());
        let task = cx.background_spawn(async move {
            let vars = backend.variables(&ws, folder.as_deref(), env.as_deref());
            (vars, backend.refs(&ws, calls))
        });
        cx.spawn(async move |this, cx| {
            let (vars, refs) = task.await;
            this.update(cx, |this, cx| {
                this.vars = Rc::new(vars);
                this.refs = Rc::new(refs);
                cx.notify();
            })
        })
        .detach();
    }

    /// Templates referenced by the request as currently edited.
    pub fn used_vars(&self, cx: &App) -> Vec<String> {
        let mut texts = vec![self.url.read(cx).value()];
        texts.extend(self.params.texts(cx));
        texts.extend(self.query.texts(cx));
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

/// What a chip's tooltip shows; built when it opens.
struct Card {
    /// Where it's defined (top right).
    place: String,
    /// What it sends, or why there's nothing to show (red when missing).
    value: Result<String, String>,
    note: Option<String>,
    hint: Option<String>,
}

/// Long values (tokens, JSON) are cut: the card is a glance, not a viewer.
fn clip(s: &str) -> String {
    match s.char_indices().nth(160) {
        Some((at, _)) => format!("{}…", &s[..at]),
        None => s.to_string(),
    }
}

/// The chip's text and the card's title: the label, or `request › field`
/// for a response call once the request's name is known.
fn title(ctx: &InlineTokenContext, refs: &Refs) -> SharedString {
    let token = ctx.token();
    vars::response_ref(token.id())
        .and_then(|(id, field)| Some(format!("{} › {field}", refs.names.get(&id)?).into()))
        .unwrap_or_else(|| token.label().clone())
}

fn describe(expr: &str, kind: Kind, vars: &Vars, refs: &Refs) -> Card {
    let open = vars::origin(expr, vars).map(|_| t!("⌘-click to open").into_owned());
    let name_of = |id: &str| refs.names.get(id).cloned();
    let sent_first = |name: &str| t!("%{name} is sent first", name = name).into_owned();
    if let Some((id, _)) = vars::response_ref(expr) {
        let name = name_of(&id).unwrap_or_else(|| t!("response").into_owned());
        let value = match refs.values.get(expr) {
            Some(Some(v)) => Ok(clip(v)),
            Some(None) => Err(t!("no response yet").into_owned()),
            None => Err("…".into()),
        };
        return Card {
            place: t!("response").into_owned(),
            value,
            note: Some(sent_first(&name)),
            hint: open,
        };
    }
    match vars.get(expr) {
        // Its value still reads a response that isn't there yet.
        Some(v) if kind == Kind::Function => Card {
            place: v.source.label(),
            value: Err(t!("no response yet").into_owned()),
            note: (vars::spans(&v.raw).into_iter())
                .find_map(|(_, e)| vars::response_ref(e))
                .and_then(|(id, _)| name_of(&id))
                .map(|name| sent_first(&name)),
            hint: open,
        },
        Some(v) => Card {
            place: v.source.label(),
            value: Ok(if v.secret {
                MASK.into()
            } else {
                clip(&v.value)
            }),
            note: (!v.secret && v.raw != v.value).then(|| format!("= {}", clip(&v.raw))),
            hint: open,
        },
        None if kind == Kind::Function => Card {
            place: t!("function").into_owned(),
            value: Err(t!("evaluated at send time").into_owned()),
            note: None,
            hint: None,
        },
        None => Card {
            place: t!("not defined").into_owned(),
            value: Err(t!("no value in the current environment").into_owned()),
            note: None,
            hint: open.map(|_| t!("⌘-click to create").into_owned()),
        },
    }
}

/// Inline chip, tinted by state; hovering shows its card. With ⌘ held
/// (`cmd`), one that leads somewhere looks like a link under the pointer.
fn chip(
    ctx: &InlineTokenContext,
    vars: &Rc<Vars>,
    refs: &Rc<Refs>,
    params: &[(String, Entity<InputState>)],
    cmd: bool,
    cx: &App,
) -> impl IntoElement {
    let expr = ctx.token().id().clone();
    if let Some(name) = expr.strip_prefix(':') {
        return path_chip(ctx, name, params, cx).into_any_element();
    }
    let kind = vars::kind(&expr, vars);
    let link = cmd && vars::origin(&expr, vars).is_some();
    let title = title(ctx, refs);
    let (vars, refs) = (vars.clone(), refs.clone());
    shell(
        ctx,
        kind_color(kind, cx),
        kind == Kind::Missing,
        link,
        title.clone(),
        cx,
    )
    // A tooltip, not app state: re-rendering the input on hover would reset
    // its token hover tracking.
    .tooltip(move |window, cx| {
        let c = describe(&expr, kind, &vars, &refs);
        let title = title.clone();
        Tooltip::element(move |_, cx| card(&title, &c, kind, cx)).build(window, cx)
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
    shell(ctx, color, empty, false, label.clone(), cx).tooltip(move |window, cx| {
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

/// Tinted pill around `text`; `strong` = emphasized border. A `link` (⌘
/// held, chip leads somewhere) gets the pointer and, under it, an underline.
fn shell(
    ctx: &InlineTokenContext,
    color: Hsla,
    strong: bool,
    link: bool,
    text: SharedString,
    cx: &App,
) -> Stateful<Div> {
    // Text-changing hover styles (decoration) don't apply: the text is shaped
    // before hover is known. A border is painted, so it follows the pointer.
    let label = div().min_w_0().text_ellipsis().child(text).when(link, |d| {
        d.border_b_1()
            .border_color(color.opacity(0.45))
            .group_hover("chip", |s| s.border_color(color))
    });
    div()
        .id(SharedString::from(format!(
            "chip-{}-{}",
            ctx.token().id(),
            ctx.range().start
        )))
        .group("chip")
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
        .when(link, |d| d.cursor_pointer())
        .child(label)
}

/// Title and origin, the value (or why there is none), a note, the ⌘ hint.
fn card(title: &SharedString, c: &Card, kind: Kind, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (muted, faint) = (theme.muted_foreground, ui::faint(cx));
    let value = match &c.value {
        Ok(v) => div().text_color(theme.foreground).child(v.clone()),
        Err(why) if kind == Kind::Missing => div().text_color(theme.red).child(why.clone()),
        Err(why) => div().text_color(muted).child(why.clone()),
    };
    v_flex()
        .max_w(px(360.))
        .gap_1()
        .text_xs()
        .font_family(theme.mono_font_family.clone())
        .child(
            h_flex()
                .justify_between()
                .gap_4()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(kind_color(kind, cx))
                        .child(title.clone()),
                )
                .child(div().flex_none().text_color(muted).child(c.place.clone())),
        )
        .child(value)
        .children(c.note.clone().map(|n| div().text_color(muted).child(n)))
        .children(
            c.hint
                .clone()
                .map(|h| div().pt_1().text_color(faint).child(h)),
        )
}
