//! `{{` autocomplete for every text field of a request — URL, table cells and
//! body code editors: a popup of defined variables at the cursor. Opens on `{{`
//! or ⌃space; ↑↓ choose, ↵/⇥ insert, esc hides.

use crate::{editor::RequestEditor, ui, var_view::MASK, vars};
use gpui_kit::component::{
    h_flex,
    input::{EditorState, InputEvent, InputState},
    v_flex, ActiveTheme as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_mcp::resolve::funcs::FUNCTIONS;

const MAX_ITEMS: usize = 8;

/// One sample per function, drawn once per launch so the preview doesn't
/// reshuffle on every keystroke.
static SAMPLES: std::sync::LazyLock<Vec<String>> =
    std::sync::LazyLock::new(|| FUNCTIONS.iter().map(|(_, f)| f()).collect());

/// The sample for `$name()`.
fn sample(name: &str) -> Option<&'static str> {
    let name = name.strip_prefix('$')?.strip_suffix("()")?;
    let ix = FUNCTIONS.iter().position(|(n, _)| *n == name)?;
    Some(&SAMPLES[ix])
}

/// The field the popup belongs to.
#[derive(Clone, PartialEq)]
pub enum Target {
    Line(Entity<InputState>),
    Code(Entity<EditorState>),
}

impl From<&Entity<InputState>> for Target {
    fn from(e: &Entity<InputState>) -> Self {
        Target::Line(e.clone())
    }
}

impl From<&Entity<EditorState>> for Target {
    fn from(e: &Entity<EditorState>) -> Self {
        Target::Code(e.clone())
    }
}

/// Same body for both input kinds: they share the API without a trait.
macro_rules! with_state {
    ($target:expr, $e:ident => $body:expr) => {
        match $target {
            Target::Line($e) => $body,
            Target::Code($e) => $body,
        }
    };
}

impl Target {
    /// Value, cursor and focus of the field.
    fn snapshot(&self, window: &Window, cx: &App) -> (SharedString, usize, bool) {
        with_state!(self, e => {
            let s = e.read(cx);
            (s.value(), s.cursor(), s.focus_handle(cx).is_focused(window))
        })
    }

    /// Where the popup hangs: under the caret line in a code editor (the same
    /// bounds the IME panel uses — valid even while the caret blinks off),
    /// under the field otherwise.
    fn anchor(&self, window: &mut Window, cx: &mut App) -> Point<Pixels> {
        match self {
            Target::Line(e) => {
                let b = e.read(cx).input_bounds();
                point(b.left(), b.bottom() + px(2.))
            }
            Target::Code(e) => e.update(cx, |s, cx| {
                let fallback = s.input_bounds();
                let at = s.value()[..s.cursor()].encode_utf16().count();
                let b = s
                    .text_bounds()
                    .and_then(|tb| s.bounds_for_range(at..at, tb, window, cx))
                    .unwrap_or(fallback);
                point(b.left(), b.bottom() + px(2.))
            }),
        }
    }
}

pub struct Completion {
    target: Target,
    selected: usize,
    /// Cursor offset of a ⌃space trigger: what is typed after it filters.
    anchor: Option<usize>,
    /// Keeps the selected row in view as ↑↓ move past the visible ones.
    scroll: ScrollHandle,
}

/// Range to replace and the name typed so far: an open `{{ prefix`, a bare
/// `$fn` prefix, else the text typed since a manual trigger.
fn query(value: &str, cursor: usize, anchor: Option<usize>) -> Option<(usize, &str)> {
    vars::open_template(value, cursor)
        .or_else(|| vars::open_function(value, cursor))
        .or_else(|| {
            let at = anchor?;
            let typed = value.get(at..cursor)?;
            typed.chars().all(vars::is_name_char).then_some((at, typed))
        })
}

/// End of the text to replace: past a `}}` right after the caret (the code
/// editor auto-closes `{{`), so inserting never leaves `{{ X }}}}`.
fn closing_end(value: &str, cursor: usize) -> usize {
    let rest = &value[cursor..];
    let ws = rest.len() - rest.trim_start_matches(' ').len();
    if rest[ws..].starts_with("}}") {
        cursor + ws + 2
    } else {
        cursor
    }
}

impl RequestEditor {
    /// Called on every change of a field that takes templates.
    pub(crate) fn update_completion(&mut self, target: impl Into<Target>, cx: &mut Context<Self>) {
        let target = target.into();
        let anchor = self
            .complete
            .as_ref()
            .filter(|c| c.target == target)
            .and_then(|c| c.anchor);
        let open = with_state!(&target, e => {
            let s = e.read(cx);
            query(&s.value(), s.cursor(), anchor).is_some()
        });
        self.complete = open.then(|| Completion {
            target,
            selected: 0,
            anchor,
            scroll: ScrollHandle::new(),
        });
        cx.notify();
    }

    /// ⌃space: list every variable at the cursor of the focused field.
    pub(crate) fn open_completion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut targets: Vec<Target> = vec![(&self.url).into()];
        targets.extend(self.params.inputs().map(Target::Line));
        targets.extend(self.headers.inputs().map(Target::Line));
        targets.extend(self.body.fields.inputs().map(Target::Line));
        targets.extend(self.body.code_editors().map(Target::Code));
        let Some((target, start)) = targets.into_iter().find_map(|t| {
            let (value, cursor, focused) = t.snapshot(window, cx);
            // The name already typed before the caret filters (and gets replaced).
            let start = value[..cursor].trim_end_matches(vars::is_name_char).len();
            focused.then_some((t, start))
        }) else {
            return;
        };
        self.complete = Some(Completion {
            target,
            selected: 0,
            anchor: Some(start),
            scroll: ScrollHandle::new(),
        });
        cx.notify();
    }

    /// The focused field and its suggestions (variables, then functions), when
    /// the popup should show. A `$` prefix asks for functions only, matched on
    /// the rest (`$em` → `$fake.email()`).
    fn suggestions(&self, window: &Window, cx: &App) -> Option<(Target, Vec<String>)> {
        let c = self.complete.as_ref()?;
        let (value, cursor, focused) = c.target.snapshot(window, cx);
        if !focused {
            return None;
        }
        let (_, prefix) = query(&value, cursor, c.anchor)?;
        let sigil = prefix.starts_with('$');
        let p = prefix.trim_start_matches('$').to_lowercase();
        let fns = FUNCTIONS
            .iter()
            .filter(|(n, _)| n.to_lowercase().contains(&p))
            .map(|(n, _)| format!("${n}()"));
        let vars = if sigil {
            vec![]
        } else {
            vars::suggest(&self.vars, prefix)
        };
        let names: Vec<String> = vars
            .into_iter()
            .map(|(k, _)| k.clone())
            .chain(fns)
            .collect();
        (!names.is_empty()).then(|| (c.target.clone(), names))
    }

    pub(crate) fn move_completion(
        &mut self,
        delta: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_, names)) = self.suggestions(window, cx) else {
            return;
        };
        if let Some(c) = self.complete.as_mut() {
            c.selected = (c.selected as isize + delta).rem_euclid(names.len() as isize) as usize;
            c.scroll.scroll_to_item(c.selected);
        }
        cx.notify();
    }

    pub(crate) fn dismiss_completion(&mut self, cx: &mut Context<Self>) {
        self.complete = None;
        cx.notify();
    }

    pub(crate) fn accept_completion(
        &mut self,
        pick: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((target, names)) = self.suggestions(window, cx) else {
            return;
        };
        let c = self.complete.take();
        let ix = pick.or(c.as_ref().map(|c| c.selected)).unwrap_or(0);
        let name = &names[ix.min(names.len() - 1)];
        let anchor = c.and_then(|c| c.anchor);
        let template = format!("{{{{ {name} }}}}");
        match target {
            // Rebuilt whole so the template becomes a chip; then the same path
            // as typing (chips, dirty state, blank-row promotion).
            Target::Line(e) => e.update(cx, |s, cx| {
                let (value, cursor) = (s.value(), s.cursor());
                let Some((start, _)) = query(&value, cursor, anchor) else {
                    return;
                };
                let head = format!("{}{template}", &value[..start]);
                let end = head.len();
                let text = format!("{head}{}", &value[closing_end(&value, cursor)..]);
                s.set_value(vars::content(&text), window, cx);
                s.set_selected_range(end..end, cx);
                cx.emit(InputEvent::Change);
            }),
            // An in-place edit: `set_value` would scroll a long body to the top.
            Target::Code(e) => e.update(cx, |s, cx| {
                let (value, cursor) = (s.value(), s.cursor());
                let Some((start, _)) = query(&value, cursor, anchor) else {
                    return;
                };
                s.set_selected_range(start..closing_end(&value, cursor), cx);
                s.replace(template, window, cx);
            }),
        }
        cx.notify();
    }

    pub(crate) fn render_completion(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (target, names) = self.suggestions(window, cx)?;
        let (selected, scroll) = self.complete.as_ref().map_or((0, None), |c| {
            (c.selected.min(names.len() - 1), Some(c.scroll.clone()))
        });
        let at = target.anchor(window, cx);
        let theme = cx.theme();
        let (blue, magenta, fg, muted, sel) = (
            theme.blue,
            theme.magenta,
            theme.foreground,
            theme.muted_foreground,
            theme.secondary,
        );
        let faint = ui::faint(cx);
        let rows = names.into_iter().enumerate().map(|(ix, name)| {
            let var = self.vars.get(&name);
            // A variable's real value; for a function, a muted `e.g. …` — a
            // sample, not what will be sent.
            let value = match var {
                Some(v) => div().text_color(fg).text_ellipsis().child(if v.secret {
                    MASK.to_string()
                } else {
                    v.value.clone()
                }),
                None => div()
                    .flex()
                    .gap_1()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_none()
                            .text_color(faint)
                            .child(t!("e.g.").into_owned()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .italic()
                            .text_color(muted)
                            .child(sample(&name).unwrap_or_default().to_string()),
                    ),
            };
            let origin = var.map_or_else(|| t!("function").into_owned(), |v| v.source.label());
            h_flex()
                .id(ElementId::Name(format!("var-opt-{name}").into()))
                .h(px(24.))
                .flex_none()
                .px_2()
                .gap_3()
                .rounded(theme.radius)
                .cursor_pointer()
                .when(ix == selected, |d| d.bg(sel))
                .hover(|d| d.bg(sel))
                .child(
                    div()
                        .flex_none()
                        .text_color(if var.is_some() { blue } else { magenta })
                        .child(name),
                )
                .child(div().flex_1().min_w_0().child(value))
                .child(div().flex_none().text_color(faint).child(origin))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.accept_completion(Some(ix), window, cx)
                    }),
                )
        });
        let theme = cx.theme();
        // MAX_ITEMS rows of 24 px with 2 px gaps; the rest scrolls.
        let rows = v_flex()
            .id("var-complete-rows")
            .max_h(px(MAX_ITEMS as f32 * 26. - 2.))
            .gap_0p5()
            .overflow_y_scroll()
            .when_some(scroll, |d, s| d.track_scroll(&s))
            .children(rows);
        let list = v_flex()
            .w(px(440.))
            .p_1()
            .gap_0p5()
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .font_family(theme.mono_font_family.clone())
            .text_xs()
            .child(rows)
            .child(
                div()
                    .px_2()
                    .pt_1()
                    .text_color(faint)
                    .child(t!("↑↓ choose · ↵ insert · esc close")),
            );
        Some(
            deferred(
                anchored()
                    .position(at)
                    .snap_to_window_with_margin(px(8.))
                    .child(list),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }
}
