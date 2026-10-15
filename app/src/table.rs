//! Name/value table for headers, form/multipart fields and query params. The
//! last row is always blank: typing in it makes it a real row and appends a
//! new blank one.

use std::path::PathBuf;

use gpui_kit::component::{
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex, ActiveTheme as _, Sizable as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_core::{new_id, BodyField, RequestParameter};

use crate::{
    chip_templates, complete::CompleteHost as _, draft, editor::RequestEditor, ui, vars, FocusNext,
    FocusPrev,
};

#[derive(Clone, Copy, PartialEq)]
pub enum Which {
    Headers,
    Fields,
    Query,
}

struct Row {
    field: BodyField,
    name: Entity<InputState>,
    value: Entity<InputState>,
    _subs: [Subscription; 2],
}

pub struct Table {
    rows: Vec<Row>,
}

pub fn param_field(p: &RequestParameter) -> BodyField {
    BodyField {
        id: p.id.clone(),
        name: p.name.clone(),
        value: p.value.clone(),
        enabled: p.enabled,
        is_file: false,
        content_type: None,
    }
}

fn blank() -> BodyField {
    BodyField {
        id: new_id(),
        name: String::new(),
        value: String::new(),
        enabled: true,
        is_file: false,
        content_type: None,
    }
}

fn value_placeholder(is_file: bool) -> &'static str {
    if is_file {
        "/path/to/file"
    } else {
        "value"
    }
}

impl Row {
    fn new(
        which: Which,
        field: BodyField,
        window: &mut Window,
        cx: &mut Context<RequestEditor>,
    ) -> Self {
        let mut cell = |text: &str, placeholder: &'static str| {
            let input = cx.new(|cx| {
                let mut s = InputState::new(window, cx).placeholder(t!(placeholder).to_string());
                s.set_value(vars::content(&draft::one_line(text)), window, cx);
                s
            });
            let id = field.id.clone();
            let sub = cx.subscribe_in(
                &input,
                window,
                move |this: &mut RequestEditor, input, ev: &InputEvent, window, cx| {
                    if matches!(ev, InputEvent::Change) {
                        chip_templates!(input, window, cx);
                        this.update_completion(input, cx);
                        this.cell_changed(which, &id, window, cx);
                    }
                },
            );
            (input, sub)
        };
        let (name, s1) = cell(&field.name, "name");
        let (value, s2) = cell(&field.value, value_placeholder(field.is_file));
        Self {
            field,
            name,
            value,
            _subs: [s1, s2],
        }
    }

    fn current(&self, cx: &App) -> BodyField {
        BodyField {
            name: draft::cell_value(&self.name.read(cx).value(), &self.field.name),
            value: draft::cell_value(&self.value.read(cx).value(), &self.field.value),
            ..self.field.clone()
        }
    }
}

impl Table {
    pub fn new(
        which: Which,
        items: Vec<BodyField>,
        window: &mut Window,
        cx: &mut Context<RequestEditor>,
    ) -> Self {
        let mut rows: Vec<Row> = items
            .into_iter()
            .map(|f| Row::new(which, f, window, cx))
            .collect();
        rows.push(Row::new(which, blank(), window, cx));
        Self { rows }
    }

    /// Rows for `pairs` (a query handed over by the URL), above the blank row.
    pub fn append_pairs(
        &mut self,
        pairs: Vec<(String, String)>,
        window: &mut Window,
        cx: &mut Context<RequestEditor>,
    ) {
        let at = self.rows.len() - 1;
        let new: Vec<Row> = pairs
            .into_iter()
            .map(|(name, value)| {
                let field = BodyField {
                    name,
                    value,
                    ..blank()
                };
                Row::new(Which::Query, field, window, cx)
            })
            .collect();
        self.rows.splice(at..at, new);
    }

    fn filled(&self) -> &[Row] {
        &self.rows[..self.rows.len() - 1]
    }

    pub fn fields(&self, cx: &App) -> Vec<BodyField> {
        self.filled().iter().map(|r| r.current(cx)).collect()
    }

    pub fn params(&self, cx: &App) -> Vec<RequestParameter> {
        self.fields(cx)
            .into_iter()
            .map(|f| RequestParameter {
                id: f.id,
                name: f.name,
                value: f.value,
                enabled: f.enabled,
            })
            .collect()
    }

    /// Every editable cell, blank row included (for ⌃space lookup).
    pub fn inputs(&self) -> impl Iterator<Item = Entity<InputState>> + '_ {
        self.rows
            .iter()
            .flat_map(|r| [r.name.clone(), r.value.clone()])
    }

    /// Enabled rows with a name (the meta line's count).
    pub fn active_count(&self, cx: &App) -> usize {
        self.filled()
            .iter()
            .filter(|r| r.field.enabled && !r.name.read(cx).value().trim().is_empty())
            .count()
    }

    /// Cell texts whose `{{ VAR }}` get resolved (file paths never are).
    pub fn texts(&self, cx: &App) -> Vec<SharedString> {
        self.filled()
            .iter()
            .flat_map(|r| {
                let value = (!r.field.is_file).then(|| r.value.read(cx).value());
                std::iter::once(r.name.read(cx).value()).chain(value)
            })
            .collect()
    }
}

impl RequestEditor {
    fn table_mut(&mut self, which: Which) -> &mut Table {
        match which {
            Which::Headers => &mut self.headers,
            Which::Fields => &mut self.body.fields,
            Which::Query => &mut self.query,
        }
    }

    fn cell_changed(
        &mut self,
        which: Which,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .table_mut(which)
            .rows
            .last()
            .is_some_and(|r| r.field.id == id)
        {
            let row = Row::new(which, blank(), window, cx);
            self.table_mut(which).rows.push(row);
            cx.notify();
        }
        self.refresh_dirty(cx);
    }

    fn update_row(
        &mut self,
        which: Which,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Row, &mut Window, &mut Context<Self>),
    ) {
        if let Some(row) = self
            .table_mut(which)
            .rows
            .iter_mut()
            .find(|r| r.field.id == id)
        {
            f(row, window, cx);
        }
        self.refresh_dirty(cx);
        cx.notify();
    }

    fn remove_row(&mut self, which: Which, id: &str, cx: &mut Context<Self>) {
        self.table_mut(which).rows.retain(|r| r.field.id != id);
        self.refresh_dirty(cx);
        cx.notify();
    }

    /// Multipart attachments land as `file` rows just above the blank row.
    pub(crate) fn add_file_rows(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for p in paths {
            let field = BodyField {
                name: "file".into(),
                value: p.to_string_lossy().into_owned(),
                is_file: true,
                ..blank()
            };
            let row = Row::new(Which::Fields, field, window, cx);
            let rows = &mut self.body.fields.rows;
            rows.insert(rows.len() - 1, row);
        }
        self.refresh_dirty(cx);
        cx.notify();
    }

    fn pick_row_file(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                let Some(path) = paths.first() else { return };
                let path = path.to_string_lossy().into_owned();
                this.update_row(Which::Fields, &id, window, cx, |row, window, cx| {
                    row.value.update(cx, |s, cx| s.set_value(path, window, cx));
                });
            })
            .ok();
        })
        .detach();
    }

    /// `files` adds the text/file column (multipart only).
    pub(crate) fn render_table(
        &self,
        which: Which,
        files: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let (blue, cyan, red, border) = (theme.blue, theme.cyan, theme.red, theme.border);
        let (fg, muted, faint) = (theme.foreground, theme.muted_foreground, ui::faint(cx));
        let table = match which {
            Which::Headers => &self.headers,
            Which::Fields => &self.body.fields,
            Which::Query => &self.query,
        };
        let last = table.rows.len() - 1;
        let head = h_flex()
            .h(px(24.))
            .gap_2()
            .text_xs()
            .text_color(faint)
            .border_b_1()
            .border_color(border)
            .child(div().w(px(16.)))
            .child(div().w(relative(0.3)).px_2p5().child(t!("name")))
            .child(div().flex_1().px_2p5().child(t!("value")))
            .when(files, |r| r.child(div().w(px(56.)).child(t!("type"))))
            .child(div().w(px(16.)));
        let rows = table.rows.iter().enumerate().map(|(ix, row)| {
            let blank = ix == last;
            let f = &row.field;
            let id = f.id.clone();
            let el = |kind: &str| ElementId::Name(format!("{kind}-{id}").into());
            let check = Checkbox::new(el("on"))
                .checked(f.enabled)
                .tab_stop(false)
                .xsmall()
                .on_click(cx.listener({
                    let id = id.clone();
                    move |this, on: &bool, window, cx| {
                        let on = *on;
                        this.update_row(which, &id, window, cx, |r, _, _| r.field.enabled = on);
                    }
                }));
            let value_color = if f.is_file { cyan } else { fg };
            let pick = (f.is_file && !blank).then(|| {
                div()
                    .id(el("pick"))
                    .text_xs()
                    .cursor_pointer()
                    .text_color(faint)
                    .hover(|s| s.text_color(fg))
                    .child(t!("choose"))
                    .on_click(cx.listener({
                        let id = id.clone();
                        move |this, _, window, cx| this.pick_row_file(id.clone(), window, cx)
                    }))
            });
            let kind = files.then(|| {
                div().id(el("kind")).w(px(56.)).text_xs().when(!blank, |d| {
                    d.cursor_pointer()
                        .text_color(if f.is_file { cyan } else { muted })
                        .hover(|s| s.text_color(fg))
                        .child(if f.is_file { t!("file") } else { t!("text") })
                        .on_click(cx.listener({
                            let id = id.clone();
                            move |this, _, window, cx| {
                                this.update_row(which, &id, window, cx, |r, window, cx| {
                                    r.field.is_file = !r.field.is_file;
                                    let p = value_placeholder(r.field.is_file);
                                    r.value.update(cx, |s, cx| {
                                        s.set_placeholder(t!(p).to_string(), window, cx)
                                    });
                                })
                            }
                        }))
                })
            });
            let remove = div()
                .id(el("rm"))
                .w(px(16.))
                .cursor_pointer()
                .text_color(faint)
                .opacity(0.)
                .when(!blank, |d| {
                    d.group_hover("row", |s| s.opacity(1.))
                        .hover(|s| s.text_color(red))
                        .child("×")
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.remove_row(which, &id, cx)),
                        )
                });
            h_flex()
                .group("row")
                .h(px(28.))
                .gap_2()
                .border_b_1()
                .border_color(border.opacity(0.5))
                .child(div().w(px(16.)).when(!blank, |d| d.child(check)))
                .child(
                    div()
                        .w(relative(0.3))
                        .text_color(blue)
                        .when(!f.enabled, |d| d.opacity(0.45))
                        .child(
                            Input::new(&row.name)
                                .appearance(false)
                                .token(self.chip_renderer()),
                        ),
                )
                .child(
                    h_flex()
                        .flex_1()
                        .gap_2()
                        .text_color(value_color)
                        .when(!f.enabled, |d| d.opacity(0.45))
                        .child(
                            div().flex_1().child(
                                Input::new(&row.value)
                                    .appearance(false)
                                    .token(self.chip_renderer()),
                            ),
                        )
                        .children(pick),
                )
                .children(kind)
                .child(remove)
        });
        v_flex()
            .id(match which {
                Which::Headers => "table-headers",
                Which::Fields => "table-fields",
                Which::Query => "table-query",
            })
            .size_full()
            .overflow_y_scroll()
            .on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &FocusPrev, window, cx| window.focus_prev(cx))
            .child(head)
            .children(rows)
            .into_any_element()
    }
}
