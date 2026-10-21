//! The git panel's changes list (⌘⇧G): flat, or under the folders the items
//! live in. A click opens that file's diff in place of the center.

use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, IconName};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_core::{GitChange, GitFileChange};

use crate::{
    git::GitView,
    git_panel::{change_mark, section},
    req_diff, tree,
    workspace::Yuki,
};

impl Yuki {
    /// A click on a change opens its diff in the center; a second one closes it.
    fn toggle_diff(&mut self, path: String, cx: &mut Context<Self>) {
        if self.git.diff.as_ref().is_some_and(|(p, _)| *p == path) {
            self.git.diff = None;
        } else {
            self.git.diff = Some((path, None));
            self.load_file_diff(cx);
        }
        cx.notify();
    }

    /// After the repo is re-read the open diff follows; once the file is
    /// committed or reverted there's nothing left to show.
    pub(crate) fn reload_file_diff(&mut self, cx: &mut Context<Self>) {
        let still = match (&self.git.diff, &self.git.view) {
            (Some((path, _)), Some(view)) => view.changes.iter().any(|c| &c.path == path),
            _ => false,
        };
        if still {
            self.load_file_diff(cx);
        } else {
            self.git.diff = None;
        }
    }

    fn load_file_diff(&mut self, cx: &mut Context<Self>) {
        let Some(((path, _), ws)) = self
            .git
            .diff
            .clone()
            .zip(self.ws.as_ref().map(|w| w.id.clone()))
        else {
            return;
        };
        let (backend, rel) = (self.backend.clone(), path.clone());
        let task = cx.background_spawn(async move { backend.file_diff(&ws, &rel) });
        cx.spawn(async move |this, cx| {
            let text = task.await;
            _ = this.update(cx, |this, cx| {
                // Still the open one (another click may have replaced it).
                if let Some((_, slot)) = this.git.diff.as_mut().filter(|(p, _)| *p == path) {
                    *slot = Some(text.into());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// `Stone / OM` — where a change lives in the tree; empty at the root. A
    /// deleted item is gone from the tree: git read its folder from HEAD.
    fn change_folder(&self, c: &GitFileChange) -> String {
        let Some(data) = self.data.as_ref() else {
            return String::new();
        };
        let id = c.node_id.as_deref().unwrap_or("");
        let parent = data
            .requests
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.folder_id.as_deref())
            .or_else(|| {
                data.folders
                    .iter()
                    .find(|f| f.id == id)
                    .map(|f| f.folder_id.as_deref())
            })
            .unwrap_or(c.parent_id.as_deref());
        tree::folder_path(&data.folders, parent).join(" / ")
    }
}

pub fn render(yuki: &Yuki, view: &GitView, cx: &mut Context<Yuki>) -> impl IntoElement {
    if view.changes.is_empty() {
        return v_flex();
    }
    let theme = cx.theme();
    let (fg, muted, hover, active) = (
        theme.foreground,
        theme.muted_foreground,
        theme.list_hover,
        theme.list_active,
    );
    // What needs attention first (conflict, edit, delete), then additions;
    // alphabetical inside each group.
    let rank = |c: GitChange| match c {
        GitChange::Conflicted => 0,
        GitChange::Modified | GitChange::Renamed => 1,
        GitChange::Deleted => 2,
        GitChange::Added | GitChange::Untracked => 3,
    };
    let by_folder = yuki.git.by_folder;
    let mut items: Vec<_> = view
        .changes
        .iter()
        .map(|c| {
            let folder = if by_folder {
                yuki.change_folder(c)
            } else {
                String::new()
            };
            let label = yuki.git_label(c.node_id.as_deref(), &c.path);
            (folder, rank(c.change), label, c)
        })
        .collect();
    // Root first (empty path), then folders by path.
    items.sort_by(|a, b| (&a.0, a.1, &a.2).cmp(&(&b.0, b.1, &b.2)));

    let open = yuki.git.diff.as_ref();
    let mut list = v_flex();
    let mut group: Option<&str> = None;
    for (ix, (folder, _, label, c)) in items.iter().enumerate() {
        if by_folder && group != Some(folder.as_str()) {
            group = Some(folder.as_str());
            let name = if folder.is_empty() {
                t!("workspace root").into_owned()
            } else {
                folder.clone()
            };
            list = list.child(
                div()
                    .px_3()
                    .pt_2()
                    .pb_0p5()
                    .truncate()
                    .text_color(muted)
                    .child(format!("▾ {name}")),
            );
        }
        let (mark, color) = change_mark(c.change, cx);
        let selected = open.is_some_and(|(p, _)| *p == c.path);
        let path = c.path.clone();
        list = list.child(
            h_flex()
                .id(("git-change", ix))
                .h(px(22.))
                .px_3()
                .when(by_folder, |r| r.pl_5())
                .gap_2()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .when(selected, |r| r.bg(active))
                .child(div().w(px(10.)).text_color(color).child(mark))
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_color(fg)
                        .child(label.clone()),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_diff(path.clone(), cx))),
        );
    }

    let toggle = div()
        .id("git-group")
        .px_3()
        .pt_3()
        .pb_1()
        .cursor_pointer()
        .text_color(muted)
        .hover(move |s| s.text_color(fg))
        .child(if by_folder {
            t!("flat list")
        } else {
            t!("by folder")
        })
        .on_click(cx.listener(|this, _, _, cx| {
            this.git.by_folder = !this.git.by_folder;
            cx.notify();
        }));
    v_flex()
        .child(
            h_flex()
                .justify_between()
                .child(section(
                    t!("changes %{n}", n = view.changes.len()).into_owned(),
                    cx,
                ))
                .child(toggle),
        )
        .child(list)
}

/// The open change's diff, in place of the center while it's open: what it
/// is, the file, ✕, and git's hunks.
pub fn render_pane(yuki: &Yuki, cx: &mut Context<Yuki>) -> Option<AnyElement> {
    let (path, text) = yuki.git.diff.as_ref()?;
    let change = yuki
        .git
        .view
        .as_ref()?
        .changes
        .iter()
        .find(|c| &c.path == path)?;
    let (mark, color) = change_mark(change.change, cx);
    let label = yuki.git_label(change.node_id.as_deref(), path);
    let theme = cx.theme();
    let (fg, muted, border, mono) = (
        theme.foreground,
        theme.muted_foreground,
        theme.border,
        theme.mono_font_family.clone(),
    );
    let body = match text {
        None => vec![div().px_2().text_color(muted).child("…")],
        Some(t) if t.trim().is_empty() => {
            vec![div().px_2().text_color(muted).child(t!("no differences"))]
        }
        Some(t) => req_diff::lines(t, cx),
    };
    let header = h_flex()
        .h(px(30.))
        .flex_none()
        .px_3()
        .gap_2()
        .border_b_1()
        .border_color(border)
        .child(div().text_color(color).child(mark))
        .child(div().flex_1().truncate().text_color(fg).child(label))
        .child(div().text_color(muted).truncate().child(path.clone()))
        .child(
            div()
                .id("git-diff-close")
                .cursor_pointer()
                .text_color(muted)
                .hover(move |s| s.text_color(fg))
                .child(IconName::Close)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.git.diff = None;
                    cx.notify();
                })),
        );
    Some(
        v_flex()
            .size_full()
            .text_xs()
            .font_family(mono)
            .child(header)
            .child(
                v_flex()
                    .id("git-diff-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .py_1()
                    .children(body),
            )
            .into_any_element(),
    )
}
