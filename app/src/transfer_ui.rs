//! Window side of import/export: pick the file, open the screens in the
//! center, react when they finish.

use gpui_kit::*;

use crate::{
    export_view::ExportScreen, import_view::ImportScreen, transfer::TransferEvent, workspace::Yuki,
    CloseTransfer, ExportWorkspace, ImportCollection,
};

pub(crate) fn on_actions(el: Div, cx: &mut Context<Yuki>) -> Div {
    el.on_action(
        cx.listener(|this, _: &ImportCollection, window, cx| this.import_collection(window, cx)),
    )
    .on_action(
        cx.listener(|this, _: &ExportWorkspace, window, cx| this.export_workspace(window, cx)),
    )
    .on_action(cx.listener(|this, _: &CloseTransfer, window, cx| this.close_transfer(window, cx)))
}

impl Yuki {
    fn import_collection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t!("import").into_owned().into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let read = cx.background_spawn(async move {
                let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                let format = yuki_import::detect_format(&content).ok_or_else(|| {
                    t!("unknown format — Voleeo, Yaak, Postman, Insomnia, OpenAPI or Bruno")
                        .into_owned()
                })?;
                let preview = yuki_import::preview(format, &content).map_err(|e| e.to_string())?;
                Ok::<_, String>((format, content, preview))
            });
            let result = read.await;
            _ = this.update_in(cx, |this, window, cx| match result {
                Ok((format, content, preview)) => {
                    let current = this
                        .ws
                        .clone()
                        .zip(this.data.as_ref().map(|d| d.folders.clone()));
                    let backend = this.backend.clone();
                    let screen = cx.new(|cx| {
                        ImportScreen::new(backend, format, content, preview, current, window, cx)
                    });
                    let focus = screen.read(cx).focus.clone();
                    let sub = cx.subscribe_in(&screen, window, Self::on_transfer);
                    this.show_transfer(screen.into(), focus, sub, window, cx);
                }
                Err(e) => {
                    this.status = t!("can't import: %{e}", e = e).into_owned().into();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn export_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ws) = self.ws.clone() else {
            return;
        };
        let backend = self.backend.clone();
        let screen = cx.new(|cx| ExportScreen::new(backend, ws, cx));
        let focus = screen.read(cx).focus.clone();
        let sub = cx.subscribe_in(&screen, window, Self::on_transfer);
        self.show_transfer(screen.into(), focus, sub, window, cx);
    }

    fn show_transfer(
        &mut self,
        view: AnyView,
        focus: FocusHandle,
        sub: Subscription,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_settings(window, cx);
        self.close_envs(window, cx);
        focus.focus(window, cx);
        self.transfer = Some((view, sub));
        cx.notify();
    }

    pub(crate) fn close_transfer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.transfer.take().is_some() {
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    fn on_transfer<E>(
        &mut self,
        _: &Entity<E>,
        ev: &TransferEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let warnings = |w: &[String]| match w.len() {
            0 => String::new(),
            n => format!(" · {}", t!("%{n} warnings", n = n)),
        };
        match ev {
            TransferEvent::Close => {}
            TransferEvent::Imported(done) => {
                self.status = format!(
                    "{}{}",
                    t!("%{n} requests imported", n = done.requests),
                    warnings(&done.warnings)
                )
                .into();
                let ws = done.workspace.clone();
                if self.ws.as_ref().is_some_and(|w| w.id == ws.id) {
                    self.reload_tree(window, cx);
                } else {
                    if !self.workspaces.iter().any(|w| w.id == ws.id) {
                        self.workspaces.push(ws.clone());
                    }
                    self.open_workspace(ws, window, cx);
                }
            }
            TransferEvent::Exported(path, w) => {
                self.status = format!(
                    "{}{}",
                    t!("exported to %{path}", path = path.display()),
                    warnings(w)
                )
                .into();
            }
        }
        self.close_transfer(window, cx);
    }

    /// Into the open workspace: tree and envs reload, open tabs stay.
    fn reload_tree(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ws_id) = self.ws.as_ref().map(|w| w.id.clone()) else {
            return;
        };
        let backend = self.backend.clone();
        let load = cx.background_spawn(async move { backend.load(&ws_id) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(data) = load.await else {
                return;
            };
            _ = this.update_in(cx, |this, window, cx| {
                this.set_envs(&data.environments, None, window, cx);
                this.data = Some(data);
                this.refresh_rows();
                cx.notify();
            });
        })
        .detach();
    }
}
