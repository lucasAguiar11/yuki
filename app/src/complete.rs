//! `{{` autocomplete for template fields — a request's URL, cells, body code
//! editors and auth; a folder/workspace's auth: a popup of variables,
//! functions and fields of the workspace's latest JSON responses (inserted as
//! `$response.body(…)`). Opens on `{{`, a bare `$` or ⌃space; ↑↓ choose, ↵/⇥
//! insert, esc hides. Popup in `complete_view.rs`.

use std::{rc::Rc, sync::Arc};

use crate::{backend::Backend, capture, var_view::MASK, vars};
use gpui_kit::component::input::{EditorState, InputEvent, InputState};
use gpui_kit::*;
use yuki_mcp::resolve::funcs::FUNCTIONS;

/// One sample per function, drawn once per launch so the preview doesn't
/// reshuffle on every keystroke.
static SAMPLES: std::sync::LazyLock<Vec<String>> =
    std::sync::LazyLock::new(|| FUNCTIONS.iter().map(|(_, f)| f()).collect());

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
    pub(crate) fn anchor(&self, window: &mut Window, cx: &mut App) -> Point<Pixels> {
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

/// A scalar field of a request's latest stored response.
pub struct Field {
    pub req_id: String,
    pub req: String,
    pub path: String,
    pub value: String,
}

pub struct Completion {
    target: Target,
    pub(crate) selected: usize,
    /// Cursor offset of a ⌃space trigger: what is typed after it filters.
    anchor: Option<usize>,
    /// Keeps the selected row in view as ↑↓ move past the visible ones.
    pub(crate) scroll: ScrollHandle,
    /// Read when the popup opens, so a response sent since shows up; `None`
    /// while reading.
    fields: Option<Rc<Vec<Field>>>,
}

pub enum ItemKind {
    Var,
    Function,
    Response,
    /// `$response.`: typed in, not inserted as a template — it opens the search.
    Namespace,
}

/// One row: what goes inside `{{ }}`, and how it shows.
pub struct Item {
    pub expr: String,
    pub label: String,
    /// The real value, or (functions) a sample.
    pub value: String,
    pub origin: String,
    pub kind: ItemKind,
}

/// What the popup shows: the rows and a line on how to search further.
pub struct Shown {
    pub target: Target,
    pub items: Vec<Item>,
    pub hint: String,
}

const NAMESPACE: &str = "$response.";

/// `response.login.token` (or `response.body.…`) → `Some(["login", "token"])`:
/// the terms of a `$response` search; also `Some([])` while `resp…` is typed.
fn response_terms(p: &str) -> Option<Vec<&str>> {
    if p.len() >= 3 && "response".starts_with(p) {
        return Some(vec![]);
    }
    let rest = p.strip_prefix("response.")?;
    let rest = match rest {
        "body" => "",
        _ => rest.strip_prefix("body.").unwrap_or(rest),
    };
    Some(rest.split('.').filter(|t| !t.is_empty()).collect())
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

/// A view with template fields: owns the popup state and says what it offers.
pub trait CompleteHost: Sized + 'static {
    fn completion(&self) -> &Option<Completion>;
    fn completion_mut(&mut self) -> &mut Option<Completion>;
    fn template_vars(&self) -> &vars::Vars;
    /// Where `$response.body` suggestions come from.
    fn workspace(&self) -> (Arc<Backend>, String);
    /// Every field ⌃space may open in; the focused one wins.
    fn template_fields(&self) -> Vec<Target>;

    fn load_fields(&mut self, cx: &mut Context<Self>) {
        let (backend, ws) = self.workspace();
        let task = cx.background_spawn(async move { backend.response_fields(&ws) });
        cx.spawn(async move |this, cx| {
            let fields = Rc::new(task.await);
            _ = this.update(cx, |h, cx| {
                if let Some(c) = h.completion_mut() {
                    c.fields = Some(fields);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Called on every change of a field that takes templates.
    fn update_completion(&mut self, target: impl Into<Target>, cx: &mut Context<Self>) {
        let target = target.into();
        let prev = self.completion().as_ref().filter(|c| c.target == target);
        let anchor = prev.and_then(|c| c.anchor);
        let fields = prev.map(|c| c.fields.clone());
        let open = with_state!(&target, e => {
            let s = e.read(cx);
            query(&s.value(), s.cursor(), anchor).is_some()
        });
        let fresh = open && fields.is_none();
        *self.completion_mut() = open.then(|| Completion {
            target,
            selected: 0,
            anchor,
            scroll: ScrollHandle::new(),
            fields: fields.flatten(),
        });
        if fresh {
            self.load_fields(cx);
        }
        cx.notify();
    }

    /// ⌃space: list everything at the cursor of the focused field.
    fn open_completion(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((target, start)) = self.template_fields().into_iter().find_map(|t| {
            let (value, cursor, focused) = t.snapshot(window, cx);
            // The name already typed before the caret filters (and gets replaced).
            let start = value[..cursor].trim_end_matches(vars::is_name_char).len();
            focused.then_some((t, start))
        }) else {
            return;
        };
        *self.completion_mut() = Some(Completion {
            target,
            selected: 0,
            anchor: Some(start),
            scroll: ScrollHandle::new(),
            fields: None,
        });
        self.load_fields(cx);
        cx.notify();
    }

    /// The focused field and what to offer there, when the popup should show.
    /// A `$` prefix leaves variables out and matches the rest (`$em` →
    /// `$fake.email()`, `$token` → a response's `$.accessToken`);
    /// `$response.login.token` searches response fields only, every term
    /// matching the request name or the JSONPath.
    fn suggestions(&self, window: &Window, cx: &App) -> Option<Shown> {
        let c = self.completion().as_ref()?;
        let (value, cursor, focused) = c.target.snapshot(window, cx);
        if !focused {
            return None;
        }
        let (_, prefix) = query(&value, cursor, c.anchor)?;
        let sigil = prefix.starts_with('$');
        let p = prefix.trim_start_matches('$').to_lowercase();
        let fields = c.fields.as_deref().map_or(&[][..], |f| &f[..]);
        let field_item = |f: &Field| Item {
            expr: capture::call(&f.req_id, &f.path),
            label: format!("{} › {}", f.req, f.path),
            value: f.value.clone(),
            origin: t!("response").into_owned(),
            kind: ItemKind::Response,
        };
        let matches = |f: &&Field, terms: &[&str]| {
            let hay = format!("{} {}", f.req, f.path).to_lowercase();
            terms.iter().all(|t| hay.contains(t))
        };
        let syntax = t!("$response.<request>.<field> narrows the response fields").into_owned();

        if let Some(terms) = response_terms(&p) {
            let items: Vec<Item> = fields
                .iter()
                .filter(|f| matches(f, &terms))
                .map(field_item)
                .collect();
            let hint = match (&c.fields, items.is_empty()) {
                (None, _) => t!("reading responses…").into_owned(),
                (Some(all), true) if all.is_empty() => {
                    t!("no JSON response yet: send a request first").into_owned()
                }
                (Some(_), true) => t!("no response field matches").into_owned(),
                (Some(_), false) => syntax,
            };
            return Some(Shown {
                target: c.target.clone(),
                items,
                hint,
            });
        }

        let vars = if sigil {
            vec![]
        } else {
            vars::suggest(self.template_vars(), prefix)
        };
        let vars = vars.into_iter().map(|(k, v)| Item {
            expr: k.clone(),
            label: k.clone(),
            value: if v.secret {
                MASK.into()
            } else {
                v.value.clone()
            },
            origin: v.source.label(),
            kind: ItemKind::Var,
        });
        // The way into the response search, while what's typed could lead there.
        let namespace = ("response".starts_with(&p) && !fields.is_empty()).then(|| Item {
            expr: NAMESPACE.into(),
            label: format!("{NAMESPACE}…"),
            value: t!("search the fields of the last responses").into_owned(),
            origin: t!("response").into_owned(),
            kind: ItemKind::Namespace,
        });
        let fns = FUNCTIONS
            .iter()
            .zip(SAMPLES.iter())
            .filter(|((n, _), _)| n.to_lowercase().contains(&p))
            .map(|((n, _), sample)| Item {
                expr: format!("${n}()"),
                label: format!("${n}()"),
                value: sample.clone(),
                origin: t!("function").into_owned(),
                kind: ItemKind::Function,
            });
        let terms: Vec<&str> = p.split('.').filter(|t| !t.is_empty()).collect();
        let found = fields
            .iter()
            .filter(|f| !terms.is_empty() && matches(f, &terms))
            .map(field_item);
        let items: Vec<Item> = vars.chain(namespace).chain(fns).chain(found).collect();
        (!items.is_empty()).then(|| Shown {
            target: c.target.clone(),
            items,
            hint: syntax,
        })
    }

    fn move_completion(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(n) = self
            .suggestions(window, cx)
            .map(|s| s.items.len())
            .filter(|&n| n > 0)
        else {
            return;
        };
        if let Some(c) = self.completion_mut() {
            c.selected = (c.selected as isize + delta).rem_euclid(n as isize) as usize;
            c.scroll.scroll_to_item(c.selected);
        }
        cx.notify();
    }

    fn dismiss_completion(&mut self, cx: &mut Context<Self>) {
        *self.completion_mut() = None;
        cx.notify();
    }

    fn accept_completion(
        &mut self,
        pick: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Shown { target, items, .. }) = self.suggestions(window, cx) else {
            return;
        };
        if items.is_empty() {
            return;
        }
        let c = self.completion_mut().take();
        let ix = pick.or(c.as_ref().map(|c| c.selected)).unwrap_or(0);
        let item = &items[ix.min(items.len() - 1)];
        let anchor = c.and_then(|c| c.anchor);
        // The namespace row types `$response.` so the search goes on.
        let template = match item.kind {
            ItemKind::Namespace => item.expr.clone(),
            _ => format!("{{{{ {} }}}}", item.expr),
        };
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
}

#[cfg(test)]
mod tests {
    use super::response_terms;

    #[test]
    fn response_search_terms() {
        assert_eq!(response_terms("resp"), Some(vec![]));
        assert_eq!(response_terms("response."), Some(vec![]));
        assert_eq!(
            response_terms("response.login.token"),
            Some(vec!["login", "token"])
        );
        assert_eq!(response_terms("response.body.token"), Some(vec!["token"]));
        assert_eq!(response_terms("re"), None);
        assert_eq!(response_terms("token"), None);
    }
}
