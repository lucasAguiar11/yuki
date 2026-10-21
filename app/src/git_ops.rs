//! The git panel's actions, each one `git_op` over `yuki_git`.

use gpui_kit::component::{button::ButtonVariant, dialog::AlertDialog, WindowExt as _};
use gpui_kit::*;
use yuki_core::GitChange;

use crate::{
    git::{init, GitOp},
    workspace::Yuki,
};

impl Yuki {
    pub fn git_init(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.git_op("creating repository…", false, Box::new(init), window, cx);
    }

    pub(crate) fn git_commit(
        &mut self,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(i) = &self.git.inputs {
            i.message.update(cx, |s, cx| s.set_value("", window, cx));
        }
        self.git.has_message = false;
        let op: GitOp = Box::new(move |dir| {
            yuki_git::stage_all(dir)?;
            let c = yuki_git::commit(dir, &message, None)?;
            Ok(Some(format!("commit {} · {}", c.short_id, c.summary)))
        });
        self.git_op("committing…", false, op, window, cx);
    }

    pub fn git_commit_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = &self.git.inputs else { return };
        let message = i.message.read(cx).value().trim().to_string();
        if message.is_empty() {
            self.git.error = t!("write the commit message").into_owned().into();
            cx.notify();
            return;
        }
        self.git_commit(message, window, cx);
    }

    pub(crate) fn git_new_branch(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(i) = &self.git.inputs {
            i.branch.update(cx, |s, cx| s.set_value("", window, cx));
        }
        let op: GitOp = Box::new(move |dir| {
            yuki_git::create_branch(dir, &name)?;
            Ok(Some(t!("branch %{name} created", name = name).into_owned()))
        });
        self.git_op("creating branch…", false, op, window, cx);
    }

    pub fn git_checkout(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let op: GitOp = Box::new(move |dir| {
            yuki_git::checkout_branch(dir, &name)?;
            Ok(Some(t!("on branch %{name}", name = name).into_owned()))
        });
        self.git_op("switching branch…", true, op, window, cx);
    }

    pub(crate) fn git_set_remote(
        &mut self,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let op: GitOp = Box::new(move |dir| {
            yuki_git::set_remote(dir, "origin", &url)?;
            Ok(Some(t!("remote origin set").into_owned()))
        });
        self.git_op("setting remote…", false, op, window, cx);
    }

    pub(crate) fn git_set_identity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = &self.git.inputs else { return };
        let name = i.name.read(cx).value().trim().to_string();
        let email = i.email.read(cx).value().trim().to_string();
        if name.is_empty() || email.is_empty() {
            self.git.error = t!("fill in name and e-mail").into_owned().into();
            cx.notify();
            return;
        }
        let op: GitOp = Box::new(move |dir| {
            yuki_git::set_identity(dir, &name, &email)?;
            Ok(Some(t!("commit author saved").into_owned()))
        });
        self.git_op("saving…", false, op, window, cx);
    }

    pub fn git_identity_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.git_set_identity(window, cx);
    }

    pub fn git_pull(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let op: GitOp = Box::new(|dir| {
            let r = yuki_git::pull(dir, None)?;
            Ok(Some(
                if r.conflicted {
                    t!("pull with conflicts — pick a side in each one")
                } else if r.up_to_date {
                    t!("already up to date")
                } else {
                    t!("updated from remote")
                }
                .into_owned(),
            ))
        });
        self.git_op("pulling…", true, op, window, cx);
    }

    pub fn git_push(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let op: GitOp = Box::new(|dir| {
            yuki_git::push(dir, None)?;
            Ok(Some(t!("pushed to remote").into_owned()))
        });
        self.git_op("pushing…", false, op, window, cx);
    }

    /// Settle one conflict with a side's whole file (`None` = that side deleted it).
    pub fn git_resolve(
        &mut self,
        path: String,
        side: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let op: GitOp = Box::new(move |dir| {
            match side {
                Some(text) => yuki_git::resolve(dir, &path, &text)?,
                None => yuki_git::resolve_delete(dir, &path)?,
            }
            Ok(None)
        });
        self.git_op("resolving…", true, op, window, cx);
    }

    pub fn git_finish_merge(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let op: GitOp = Box::new(|dir| {
            let c = yuki_git::finish_merge(dir, "Merge remote changes", None)?;
            Ok(Some(t!("merge done · %{id}", id = c.short_id).into_owned()))
        });
        self.git_op("finishing merge…", true, op, window, cx);
    }

    /// Right-click → discard: the file back to the last commit. Asks first —
    /// there's no undo, and a file git never saw is deleted from disk.
    pub(crate) fn confirm_discard(
        &mut self,
        path: String,
        label: String,
        change: GitChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let detail: SharedString = match change {
            GitChange::Added | GitChange::Untracked => {
                t!("it was never committed: its file will be deleted from disk.")
            }
            GitChange::Deleted => t!("it comes back as it was in the last commit."),
            _ => t!("it goes back to the last commit; these edits are lost."),
        }
        .into_owned()
        .into();
        let title: SharedString = t!("discard the changes to “%{name}”?", name = label)
            .into_owned()
            .into();
        let this = cx.entity().downgrade();
        let build = move |alert: AlertDialog, _: &mut Window, _: &mut App| {
            let (this, path) = (this.clone(), path.clone());
            alert
                .title(title.clone())
                .description(detail.clone())
                .show_cancel(true)
                .ok_text(t!("discard").to_string())
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(t!("cancel").to_string())
                .on_ok(move |_, window, cx| {
                    let path = path.clone();
                    _ = this.update(cx, |this, cx| {
                        let op: GitOp = Box::new(move |dir| {
                            yuki_git::discard(dir, std::slice::from_ref(&path))?;
                            Ok(Some(t!("changes discarded").into_owned()))
                        });
                        // Reload: the tree and open tabs show the file as it is now.
                        this.git_op("discarding…", true, op, window, cx);
                    });
                    true
                })
        };
        // Deferred and from root focus, like `confirm_delete`: the menu that
        // opened this restores its own focus afterwards.
        let root = self.focus.clone();
        window.defer(cx, move |window, cx| {
            root.focus(window, cx);
            window.open_alert_dialog(cx, build);
        });
    }
}
