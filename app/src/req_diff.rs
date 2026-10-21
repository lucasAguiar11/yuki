//! The `diff` pane of a request: its `req_{id}.yaml` (and notes) against the
//! last commit, as git prints it. Offered in the meta line while git marks the
//! request changed; re-read whenever the repo state is (`Yuki::refresh_git`).

use gpui_kit::component::{v_flex, ActiveTheme as _};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::editor::{ReqTab, RequestEditor};

impl RequestEditor {
    /// Git's verdict for this request changed (or was re-read).
    pub(crate) fn set_git_changed(&mut self, changed: bool, cx: &mut Context<Self>) {
        self.git_changed = changed;
        if self.req_tab == ReqTab::Diff {
            if changed {
                self.load_diff(cx);
            } else {
                // Committed or reverted: nothing left to show.
                self.req_tab = ReqTab::Headers;
            }
        }
        cx.notify();
    }

    pub(crate) fn load_diff(&mut self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let (ws, id) = (self.saved.workspace_id.clone(), self.saved.id.clone());
        let task = cx.background_spawn(async move { backend.request_diff(&ws, &id) });
        cx.spawn(async move |this, cx| {
            let diff = task.await;
            _ = this.update(cx, |this, cx| {
                this.file_diff = Some(diff.into());
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn render_diff(&self, cx: &mut Context<Self>) -> AnyElement {
        let text = self.file_diff.clone().unwrap_or_default();
        v_flex()
            .id("req-diff")
            .size_full()
            .overflow_y_scroll()
            .py_1()
            .font_family(cx.theme().mono_font_family.clone())
            .text_xs()
            .children(lines(&text, cx))
            .into_any_element()
    }
}

/// Git's patch text as rows: `+` tinted green, `-` red, the rest muted.
pub fn lines(text: &str, cx: &App) -> Vec<Div> {
    let theme = cx.theme();
    let (fg, muted, green, red) = (
        theme.foreground,
        theme.muted_foreground,
        theme.green,
        theme.red,
    );
    text.lines()
        .map(|l| {
            let (bg, color) = match l.as_bytes().first() {
                Some(b'+') => (Some(green.opacity(0.12)), fg),
                Some(b'-') => (Some(red.opacity(0.12)), fg),
                _ => (None, muted),
            };
            div()
                .px_2()
                .text_color(color)
                .when_some(bg, |d, bg| d.bg(bg))
                .child(if l.is_empty() {
                    " ".to_string()
                } else {
                    l.to_string()
                })
        })
        .collect()
}
