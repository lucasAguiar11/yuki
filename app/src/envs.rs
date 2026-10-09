//! Environments on the app side: the `env` select in the title bar and the
//! "Ambientes" screen (`env_screen.rs`) that edits them.

use gpui_kit::component::{
    searchable_list::SearchableVec,
    select::{SelectEvent, SelectState},
    IndexPath,
};
use gpui_kit::*;
use yuki_core::{Environment, EnvironmentKind};

use crate::{
    env_screen::{EnvScreen, EnvsChanged},
    workspace::Yuki,
};

impl Yuki {
    /// Rebuild the title select from `envs`, keeping `active` (or the current
    /// pick) selected; with neither, the first personal env.
    pub(crate) fn set_envs(
        &mut self,
        envs: &[Environment],
        active: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keep = active.or_else(|| self.env_id(cx));
        let personal: Vec<_> = envs
            .iter()
            .filter(|e| e.kind == EnvironmentKind::Personal)
            .collect();
        let mut names = vec![SharedString::from("Sem ambiente")];
        names.extend(personal.iter().map(|e| SharedString::from(e.name.clone())));
        self.env_ids = std::iter::once(None)
            .chain(personal.iter().map(|e| Some(e.id.clone())))
            .collect();
        // Requests usually depend on one: default to the first personal env.
        let ix = keep
            .and_then(|id| self.env_ids.iter().position(|e| e.as_deref() == Some(&id)))
            .unwrap_or(if personal.is_empty() { 0 } else { 1 });
        let env = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(names),
                Some(IndexPath::new(ix)),
                window,
                cx,
            )
        });
        self.env_sub = Some(cx.subscribe_in(
            &env,
            window,
            |this, _, _: &SelectEvent<SearchableVec<SharedString>>, window, cx| {
                this.reresolve(window, cx)
            },
        ));
        self.env = Some(env);
    }

    pub(crate) fn env_id(&self, cx: &App) -> Option<String> {
        let ix = self.env.as_ref()?.read(cx).selected_index(cx)?.row;
        self.env_ids.get(ix).cloned().flatten()
    }

    /// Color of the active environment, if it has one.
    pub fn active_env_color(&self, cx: &App) -> Option<Hsla> {
        let id = self.env_id(cx)?;
        let env = self
            .data
            .as_ref()?
            .environments
            .iter()
            .find(|e| e.id == id)?;
        crate::ui::env_color(&env.color)
    }

    /// Every open request re-resolves its variables (env picked or edited).
    fn reresolve(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let env_id = self.env_id(cx);
        for tab in &self.tabs {
            tab.update(cx, |e, cx| e.set_env(env_id.clone(), window, cx));
        }
        cx.notify();
    }

    /// Show the "Ambientes" screen on the active environment (global when
    /// none); `add` starts a variable with that name there.
    pub fn open_envs(&mut self, add: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ws) = self.ws.as_ref().map(|w| w.id.clone()) else {
            return;
        };
        if let Some((screen, _)) = &self.env_screen {
            let screen = screen.clone();
            if let Some(key) = add {
                screen.update(cx, |s, cx| s.add_var(&key, window, cx));
            }
            return;
        }
        let active = self.env_id(cx);
        let backend = self.backend.clone();
        let task = cx.background_spawn({
            let ws = ws.clone();
            async move { backend.environments(&ws) }
        });
        cx.spawn_in(window, async move |this, cx| {
            let envs = task.await;
            _ = this.update_in(cx, |this, window, cx| match envs {
                Ok(envs) => {
                    let ix = active
                        .and_then(|id| envs.iter().position(|e| e.id == id))
                        .unwrap_or(0);
                    let backend = this.backend.clone();
                    let screen = cx.new(|cx| EnvScreen::new(backend, ws, envs, ix, window, cx));
                    let sub = cx.subscribe_in(&screen, window, Self::envs_changed);
                    match add {
                        Some(key) => screen.update(cx, |s, cx| s.add_var(&key, window, cx)),
                        None => {
                            let focus = screen.read(cx).focus.clone();
                            focus.focus(window, cx);
                        }
                    }
                    this.env_screen = Some((screen, sub));
                    cx.notify();
                }
                Err(e) => {
                    this.status = format!("falha ao ler ambientes: {e}").into();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn close_envs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((screen, _sub)) = self.env_screen.take() {
            screen.update(cx, |s, cx| s.flush(cx));
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    fn envs_changed(
        &mut self,
        _: &Entity<EnvScreen>,
        ev: &EnvsChanged,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(data) = self.data.as_mut() {
            data.environments = ev.envs.clone();
        }
        self.set_envs(&ev.envs, ev.activate.clone(), window, cx);
        self.reresolve(window, cx);
    }

    /// Quit: write a pending env edit synchronously.
    pub(crate) fn flush_envs_blocking(&self, cx: &App) {
        if let Some((screen, _)) = &self.env_screen {
            screen.read(cx).flush_blocking(cx);
        }
    }
}
