//! Request "notes" tab: markdown kept in `req_{id}.md` (`RequestStore` owns the
//! file), edited in a code editor or read rendered. Saved with the request
//! (`RequestEditor::save`).

use gpui_kit::base::text::TextView;
use gpui_kit::component::{h_flex, input::Editor, v_flex, ActiveTheme as _};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::editor::RequestEditor;

impl RequestEditor {
    pub(crate) fn notes_text(&self, cx: &App) -> String {
        self.notes.read(cx).value().to_string()
    }

    pub(crate) fn load_notes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let (ws, id) = (self.saved.workspace_id.clone(), self.saved.id.clone());
        let task = cx.background_spawn(async move { backend.notes(&ws, &id) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(notes) = task.await else { return };
            _ = this.update_in(cx, |this, window, cx| {
                // Typed before the read landed: keep what the user wrote.
                if !this.notes_text(cx).is_empty() {
                    return;
                }
                // Read existing notes; write new ones (mode never flips while typing).
                this.notes_preview = !notes.trim().is_empty();
                this.notes_saved = notes.clone();
                this.notes
                    .update(cx, |s, cx| s.set_value(notes, window, cx));
                cx.notify();
            });
        })
        .detach();
    }

    /// Empty notes have nothing to preview.
    fn previewing(&self, cx: &App) -> bool {
        self.notes_preview && !self.notes_text(cx).trim().is_empty()
    }

    /// `notes` / `notes —` in the meta line.
    pub(crate) fn notes_label(&self, cx: &App) -> &'static str {
        if self.notes_text(cx).trim().is_empty() {
            "notes —"
        } else {
            "notes"
        }
    }

    pub(crate) fn render_notes(&self, cx: &mut Context<Self>) -> AnyElement {
        let previewing = self.previewing(cx);
        let theme = cx.theme();
        let (fg, muted, radius, secondary, ui_font) = (
            theme.foreground,
            theme.muted_foreground,
            theme.radius,
            theme.secondary,
            theme.font_family.clone(),
        );
        let mode = |id: &'static str, label: String, preview: bool, cx: &mut Context<Self>| {
            let active = previewing == preview;
            div()
                .id(id)
                .px_1p5()
                .rounded(radius)
                .cursor_pointer()
                .text_color(if active { fg } else { muted })
                .when(active, |s| s.bg(secondary))
                .hover(|s| s.text_color(fg))
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.notes_preview = preview;
                    if !preview {
                        this.notes.read(cx).focus_handle(cx).focus(window, cx);
                    }
                    cx.notify();
                }))
        };
        let toolbar = h_flex()
            .gap_1()
            .text_xs()
            .child(div().flex_1())
            .child(mode("notes-edit", t!("edit").into_owned(), false, cx))
            .child(mode("notes-preview", t!("preview").into_owned(), true, cx));
        let content = if previewing {
            TextView::markdown("notes-md", self.notes_text(cx))
                .scrollable(true)
                .size_full()
                .px_16()
                .py_2()
                .font_family(ui_font)
                .into_any_element()
        } else {
            Editor::new(&self.notes)
                .appearance(false)
                .h_full()
                .into_any_element()
        };
        v_flex()
            .size_full()
            .gap_1()
            .child(toolbar)
            .child(div().flex_1().min_h_0().child(content))
            .into_any_element()
    }
}
