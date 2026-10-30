//! The params tab: path params (one row per `:name` in the URL, names from the
//! URL, value substituted at send) and query params as rows. The URL bar never
//! keeps a query: one pasted (or left there when it loses focus) moves to the
//! rows (`query.rs`).

use gpui_kit::component::{
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex, ActiveTheme as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{
    chip_templates,
    complete::CompleteHost as _,
    draft,
    editor::{ReqTab, RequestEditor},
    query,
    table::Which,
    ui, vars, FocusNext, FocusPrev,
};

struct Row {
    name: String,
    /// Stored value: an untouched cell keeps it verbatim (`draft::cell_value`).
    stored: String,
    value: Entity<InputState>,
    _sub: Subscription,
}

#[derive(Default)]
pub struct PathParams {
    rows: Vec<Row>,
}

impl PathParams {
    pub fn values(&self, cx: &App) -> Vec<(String, String)> {
        self.rows
            .iter()
            .map(|r| {
                let shown = r.value.read(cx).value();
                (r.name.clone(), draft::cell_value(&shown, &r.stored))
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Name → value input, for the URL chips' tooltips.
    pub fn entries(&self) -> Vec<(String, Entity<InputState>)> {
        self.rows
            .iter()
            .map(|r| (r.name.clone(), r.value.clone()))
            .collect()
    }
    pub fn inputs(&self) -> impl Iterator<Item = Entity<InputState>> + '_ {
        self.rows.iter().map(|r| r.value.clone())
    }

    pub fn texts(&self, cx: &App) -> Vec<SharedString> {
        self.rows.iter().map(|r| r.value.read(cx).value()).collect()
    }
}

impl RequestEditor {
    /// Rows follow the URL: a new `:name` gets a row, a gone one loses it.
    pub(crate) fn sync_path_params(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let names = yuki_mcp::resolve::path_params(&self.url.read(cx).value());
        if names.iter().eq(self.params.rows.iter().map(|r| &r.name)) {
            return;
        }
        let mut old = std::mem::take(&mut self.params.rows);
        for name in names {
            let row = match old.iter().position(|r| r.name == name) {
                Some(ix) => old.swap_remove(ix),
                None => self.param_row(name, window, cx),
            };
            self.params.rows.push(row);
        }
        cx.notify();
    }

    /// The URL's query, if any, becomes rows and leaves the URL bar.
    pub(crate) fn take_url_query(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.url.read(cx).value().to_string();
        if !text.contains('?') {
            return;
        }
        let pairs = query::pairs(&text);
        let moved = !pairs.is_empty();
        self.query.append_pairs(pairs, window, cx);
        let base = query::without_query(&text);
        self.url_len = base.len();
        // `set_value` emits no change event: the URL's own handler won't run.
        self.url.update(cx, |s, cx| {
            s.set_value(vars::url_content(&base), window, cx)
        });
        if moved {
            self.req_tab = ReqTab::Params;
            self.editor_open = true;
        }
        self.refresh_dirty(cx);
        cx.notify();
    }

    /// `:path` params plus enabled, named query rows (the meta line's `params N`).
    pub fn params_count(&self, cx: &App) -> usize {
        self.params.len() + self.query.active_count(cx)
    }

    fn param_row(&self, name: String, window: &mut Window, cx: &mut Context<Self>) -> Row {
        let stored = self
            .shown
            .parameters
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.value.clone())
            .unwrap_or_default();
        let value = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder(t!("value").to_string());
            s.set_value(vars::content(&draft::one_line(&stored)), window, cx);
            s
        });
        let _sub = cx.subscribe_in(
            &value,
            window,
            |this, input, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::Change) {
                    chip_templates!(input, window, cx);
                    this.update_completion(input, cx);
                    this.refresh_dirty(cx);
                }
            },
        );
        Row {
            name,
            stored,
            value,
            _sub,
        }
    }

    pub(crate) fn render_params(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (blue, fg, border, faint) = (theme.blue, theme.foreground, theme.border, ui::faint(cx));
        let head = h_flex()
            .h(px(24.))
            .gap_2()
            .text_xs()
            .text_color(faint)
            .border_b_1()
            .border_color(border)
            .child(div().w(relative(0.3)).px_2p5().child(t!("name")))
            .child(div().flex_1().px_2p5().child(t!("value")));
        let rows = self.params.rows.iter().map(|r| {
            h_flex()
                .h(px(28.))
                .gap_2()
                .border_b_1()
                .border_color(border.opacity(0.5))
                .child(
                    div()
                        .w(relative(0.3))
                        .px_2p5()
                        .truncate()
                        .text_color(blue)
                        .child(format!(":{}", r.name)),
                )
                .child(
                    div()
                        .flex_1()
                        .text_color(fg)
                        .child(self.chips(Input::new(&r.value).appearance(false))),
                )
        });
        v_flex()
            .size_full()
            .child(
                v_flex()
                    .id("table-params")
                    .flex_none()
                    .on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
                    .on_action(|_: &FocusPrev, window, cx| window.focus_prev(cx))
                    .when(!self.params.rows.is_empty(), |d| {
                        d.child(head).children(rows.collect::<Vec<_>>())
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_table(Which::Query, false, cx)),
            )
            .into_any_element()
    }
}
