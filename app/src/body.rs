//! Body pane: a kind select plus the input that kind edits — code editor
//! (syntax colors + `{{ VAR }}` chips, `var_view::pin_chips`) for raw/graphql,
//! a name/value table for form/multipart, a path for binary.

use std::path::PathBuf;

use gpui_kit::component::{
    h_flex,
    input::{Editor, EditorState, Input, InputEvent, InputState},
    searchable_list::SearchableVec,
    select::{Select, SelectEvent, SelectState},
    v_flex, ActiveTheme as _, IndexPath, Sizable as _,
};
use gpui_kit::*;
use yuki_core::{BodyKind, RequestBody};

use crate::{
    chip_templates,
    complete::CompleteHost as _,
    draft,
    editor::RequestEditor,
    table::{Table, Which},
    ui, var_view, vars,
};

const KINDS: [BodyKind; 9] = [
    BodyKind::None,
    BodyKind::Json,
    BodyKind::Xml,
    BodyKind::Text,
    BodyKind::Html,
    BodyKind::FormUrlEncoded,
    BodyKind::Multipart,
    BodyKind::Binary,
    BodyKind::Graphql,
];

type KindSelect = SelectState<SearchableVec<SharedString>>;

/// Highlighter for a raw body. XML rides the HTML grammar: tags, attributes
/// and strings parse the same and gpui-component ships no XML grammar.
fn lang(kind: &BodyKind) -> &'static str {
    match kind {
        BodyKind::Json => "json",
        BodyKind::Xml | BodyKind::Html => "html",
        BodyKind::Graphql => "graphql",
        _ => "text",
    }
}

pub struct BodyInputs {
    kind: Entity<KindSelect>,
    pub(crate) text: Entity<EditorState>,
    pub(crate) graphql_vars: Entity<EditorState>,
    pub(crate) fields: Table,
    file: Entity<InputState>,
}

impl BodyInputs {
    pub fn new(
        body: Option<&RequestBody>,
        window: &mut Window,
        cx: &mut Context<RequestEditor>,
    ) -> (Self, Vec<Subscription>) {
        let t = draft::BodyDraft::from(body);
        let names: Vec<SharedString> = KINDS.iter().map(|k| ui::kind_name(k).into()).collect();
        let ix = KINDS.iter().position(|k| *k == t.kind).map(IndexPath::new);
        let kind = cx.new(|cx| SelectState::new(SearchableVec::new(names), ix, window, cx));
        // Code editors with `{{ }}` chips, like the URL (patched gpui-base).
        let text = cx.new(|cx| {
            let mut s = EditorState::new(window, cx).language(lang(&t.kind));
            s.set_value(vars::content(&t.text), window, cx);
            s
        });
        let graphql_vars = cx.new(|cx| {
            let mut s = EditorState::new(window, cx)
                .language("json")
                .placeholder("{ \"id\": 1 }");
            s.set_value(vars::content(&t.graphql_vars), window, cx);
            s
        });
        let fields = Table::new(Which::Fields, t.fields, window, cx);
        let file = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder(t!("/path/to/file").into_owned());
            s.set_value(t.file, window, cx);
            s
        });
        let subs = vec![
            cx.subscribe_in(
                &kind,
                window,
                |this, _, _: &SelectEvent<SearchableVec<SharedString>>, _, cx| {
                    let lang = lang(&this.body.kind(cx));
                    this.body
                        .text
                        .update(cx, |s, cx| s.set_highlighter(lang, cx));
                    this.refresh_dirty(cx);
                    cx.notify();
                },
            ),
            cx.subscribe_in(&text, window, Self::on_code_input),
            cx.subscribe_in(&graphql_vars, window, Self::on_code_input),
            cx.subscribe_in(&file, window, RequestEditor::on_input),
        ];
        let inputs = Self {
            kind,
            text,
            graphql_vars,
            fields,
            file,
        };
        (inputs, subs)
    }

    fn on_code_input(
        this: &mut RequestEditor,
        editor: &Entity<EditorState>,
        ev: &InputEvent,
        window: &mut Window,
        cx: &mut Context<RequestEditor>,
    ) {
        if matches!(ev, InputEvent::Change) {
            chip_templates!(editor, window, cx);
            this.update_completion(editor, cx);
            this.refresh_dirty(cx);
        }
    }

    pub fn code_editors(&self) -> [Entity<EditorState>; 2] {
        [self.text.clone(), self.graphql_vars.clone()]
    }

    pub fn kind(&self, cx: &App) -> BodyKind {
        let name = self.kind.read(cx).selected_value();
        KINDS
            .into_iter()
            .find(|k| name.is_some_and(|n| ui::kind_name(k) == n.as_ref()))
            .unwrap_or_default()
    }

    pub fn draft(&self, cx: &App) -> draft::BodyDraft {
        draft::BodyDraft {
            kind: self.kind(cx),
            text: self.text.read(cx).value().to_string(),
            graphql_vars: self.graphql_vars.read(cx).value().to_string(),
            fields: self.fields.fields(cx),
            file: self.file.read(cx).value().to_string(),
        }
    }

    /// Texts whose `{{ VAR }}` get resolved for the current kind.
    pub fn template_texts(&self, cx: &App) -> Vec<SharedString> {
        match self.kind(cx) {
            BodyKind::None | BodyKind::Binary => vec![],
            BodyKind::FormUrlEncoded | BodyKind::Multipart => self.fields.texts(cx),
            BodyKind::Graphql => vec![
                self.text.read(cx).value(),
                self.graphql_vars.read(cx).value(),
            ],
            _ => vec![self.text.read(cx).value()],
        }
    }
}

impl RequestEditor {
    pub(crate) fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let b = &self.body;
        let kind = b.kind(cx);
        for s in [&b.text, &b.graphql_vars] {
            var_view::pin_chips(s, self.vars.clone(), self.refs.clone(), cx);
        }
        let editor = |s: &Entity<EditorState>| Editor::new(s).appearance(false).h_full();
        let content = match kind {
            BodyKind::None => div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(t!("no body"))
                .into_any_element(),
            BodyKind::FormUrlEncoded | BodyKind::Multipart => {
                self.render_table(Which::Fields, kind == BodyKind::Multipart, cx)
            }
            BodyKind::Binary => Input::new(&b.file).into_any_element(),
            BodyKind::Graphql => {
                let (query, vars) = (editor(&b.text), editor(&b.graphql_vars));
                h_flex()
                    .size_full()
                    .gap_4()
                    .child(div().flex_1().h_full().child(query))
                    .child(
                        v_flex()
                            .w(relative(0.4))
                            .h_full()
                            .pl_4()
                            .border_l_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("variables"),
                            )
                            .child(div().flex_1().min_h_0().child(vars)),
                    )
                    .into_any_element()
            }
            _ => editor(&b.text).into_any_element(),
        };
        let pick = match kind {
            BodyKind::Binary => Some(t!("choose file")),
            BodyKind::Multipart => Some(t!("+ file")),
            _ => None,
        };
        let pick = pick.map(|label| {
            div()
                .id("body-pick")
                .cursor_pointer()
                .hover(|s| s.text_color(cx.theme().foreground))
                .child(label)
                .on_click(cx.listener(|this, _, window, cx| this.pick_files(window, cx)))
        });
        let format = (kind == BodyKind::Json).then(|| {
            div()
                .id("body-format")
                .cursor_pointer()
                .hover(|s| s.text_color(cx.theme().foreground))
                .child(t!("format"))
                .on_click(cx.listener(|this, _, window, cx| this.format_body(window, cx)))
        });
        let theme = cx.theme();
        v_flex()
            .size_full()
            .gap_2()
            .child(
                h_flex()
                    .gap_4()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(
                        div()
                            .w(px(150.))
                            .child(Select::new(&b.kind).appearance(false).xsmall()),
                    )
                    .children(pick)
                    .children(format),
            )
            .child(div().flex_1().min_h_0().child(content))
            .into_any_element()
    }

    /// Re-indents the JSON body as one undoable edit (⌘Z restores it); left
    /// as typed when it doesn't balance.
    fn format_body(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.body.text.update(cx, |s, cx| {
            let value = s.value();
            let Some(pretty) = draft::pretty_json(&value).filter(|p| *p != value) else {
                return;
            };
            s.set_selected_range(0..value.len(), cx);
            s.replace(pretty, window, cx);
        });
    }

    /// Binary takes one file as its path; multipart adds one `file` row each.
    fn pick_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let multiple = self.body.kind(cx) == BodyKind::Multipart;
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| this.attach(paths, window, cx))
                .ok();
        })
        .detach();
    }

    fn attach(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        if self.body.kind(cx) != BodyKind::Binary {
            return self.add_file_rows(paths, window, cx);
        }
        if let Some(p) = paths.last() {
            let p = p.to_string_lossy().into_owned();
            self.body
                .file
                .update(cx, |s, cx| s.set_value(p, window, cx));
        }
        self.refresh_dirty(cx);
    }
}
