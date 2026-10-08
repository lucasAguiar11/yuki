//! The git panel's actions, each one `git_op` over `yuki_git`.

use gpui_kit::*;

use crate::{
    git::{init, GitOp},
    workspace::Yuki,
};

impl Yuki {
    pub fn git_init(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.git_op("criando repositório…", false, Box::new(init), window, cx);
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
        self.git_op("commitando…", false, op, window, cx);
    }

    pub fn git_commit_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = &self.git.inputs else { return };
        let message = i.message.read(cx).value().trim().to_string();
        if message.is_empty() {
            self.git.error = "escreva a mensagem do commit".into();
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
            Ok(Some(format!("branch {name} criada")))
        });
        self.git_op("criando branch…", false, op, window, cx);
    }

    pub fn git_checkout(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let op: GitOp = Box::new(move |dir| {
            yuki_git::checkout_branch(dir, &name)?;
            Ok(Some(format!("na branch {name}")))
        });
        self.git_op("trocando de branch…", true, op, window, cx);
    }

    pub(crate) fn git_set_remote(
        &mut self,
        url: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let op: GitOp = Box::new(move |dir| {
            yuki_git::set_remote(dir, "origin", &url)?;
            Ok(Some("remote origin configurado".into()))
        });
        self.git_op("configurando remote…", false, op, window, cx);
    }

    pub(crate) fn git_set_identity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = &self.git.inputs else { return };
        let name = i.name.read(cx).value().trim().to_string();
        let email = i.email.read(cx).value().trim().to_string();
        if name.is_empty() || email.is_empty() {
            self.git.error = "preencha nome e e-mail".into();
            cx.notify();
            return;
        }
        let op: GitOp = Box::new(move |dir| {
            yuki_git::set_identity(dir, &name, &email)?;
            Ok(Some("autor dos commits salvo".into()))
        });
        self.git_op("salvando…", false, op, window, cx);
    }

    pub fn git_identity_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.git_set_identity(window, cx);
    }

    pub fn git_pull(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let op: GitOp = Box::new(|dir| {
            let r = yuki_git::pull(dir, None)?;
            Ok(Some(
                if r.conflicted {
                    "pull com conflitos — escolha um lado em cada um"
                } else if r.up_to_date {
                    "já está atualizado"
                } else {
                    "atualizado do remote"
                }
                .into(),
            ))
        });
        self.git_op("puxando…", true, op, window, cx);
    }

    pub fn git_push(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let op: GitOp = Box::new(|dir| {
            yuki_git::push(dir, None)?;
            Ok(Some("enviado para o remote".into()))
        });
        self.git_op("enviando…", false, op, window, cx);
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
        self.git_op("resolvendo…", true, op, window, cx);
    }

    pub fn git_finish_merge(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let op: GitOp = Box::new(|dir| {
            let c = yuki_git::finish_merge(dir, "Merge remote changes", None)?;
            Ok(Some(format!("merge concluído · {}", c.short_id)))
        });
        self.git_op("concluindo merge…", true, op, window, cx);
    }
}
