//! The git panel's actions, each one `git_op` over `yuki_git`.

use gpui_kit::*;

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
}
