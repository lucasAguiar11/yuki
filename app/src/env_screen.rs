//! "Ambientes" screen (⌘⌥E): pick an environment on the left, register its
//! variables on the right. Edits go to disk 400 ms after typing stops, like
//! requests. Secrets are plaintext here; encrypted workspaces seal them on save.

use std::{sync::Arc, time::Duration};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use yuki_core::{Environment, EnvironmentKind, EnvironmentVariable, YukiError};
use yuki_mcp::api::ApiBackend;

use crate::{backend::Backend, secrets};

const AUTOSAVE_DELAY: Duration = Duration::from_millis(400);

pub struct VarRow {
    pub key: Entity<InputState>,
    pub value: Entity<InputState>,
    pub secret: bool,
    pub enabled: bool,
    _subs: [Subscription; 2],
}

/// The name box: creating a new environment or renaming the selected one.
pub enum Naming {
    New,
    Rename,
}

/// Emitted after a save, create, rename or delete: the app re-reads the env
/// list (title select) and re-resolves open requests. `activate` = make this
/// the active environment.
pub struct EnvsChanged {
    pub envs: Vec<Environment>,
    pub activate: Option<String>,
}

pub struct EnvScreen {
    backend: Arc<Backend>,
    ws_id: String,
    /// Plaintext copies, global first.
    pub envs: Vec<Environment>,
    pub selected: usize,
    /// Rows of the selected env; the last one is always blank.
    pub rows: Vec<VarRow>,
    pub naming: Option<(Naming, Entity<InputState>)>,
    pub focus: FocusHandle,
    pub error: SharedString,
    autosave: Option<Task<()>>,
    /// Secrets are ciphertext on disk only in encrypted workspaces.
    pub encrypted_ws: bool,
    saving: bool,
    _naming_sub: Option<Subscription>,
}

impl EventEmitter<EnvsChanged> for EnvScreen {}

impl Backend {
    /// Every environment of `ws_id`, secrets opened; the global one is created
    /// on first use.
    pub fn environments(&self, ws_id: &str) -> Result<Vec<Environment>, YukiError> {
        let store = &self.api.environments;
        store.ensure_global(ws_id)?;
        let mut envs = store.list(ws_id)?;
        if let Some(key) = secrets::key(&self.api, ws_id)? {
            for env in &mut envs {
                secrets::open_vars(&mut env.variables, &key);
            }
        }
        Ok(envs)
    }

    /// Write `env` if it differs from disk; secrets sealed on encrypted workspaces.
    pub fn save_env(&self, env: &Environment) -> Result<(), YukiError> {
        save_env(&self.api, env)
    }
}

fn save_env(api: &ApiBackend, env: &Environment) -> Result<(), YukiError> {
    let store = &api.environments;
    let stored = store.get(&env.workspace_id, &env.id)?;
    let mut next = env.clone();
    if let Some(key) = secrets::key(api, &env.workspace_id)? {
        let old = stored.as_ref().map_or(&[][..], |s| &s.variables[..]);
        secrets::seal_vars(&mut next.variables, old, &key)?;
    }
    if let Some(s) = &stored {
        next.updated_at = s.updated_at.clone();
        if *s == next {
            return Ok(());
        }
    }
    next.updated_at = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.6f")
        .to_string();
    store.save(&next)
}

impl EnvScreen {
    pub fn new(
        backend: Arc<Backend>,
        ws_id: String,
        envs: Vec<Environment>,
        selected: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let encrypted_ws = backend
            .api
            .workspaces
            .get(&ws_id)
            .is_ok_and(|w| w.encrypted);
        let mut this = Self {
            backend,
            ws_id,
            envs,
            selected: 0,
            rows: vec![],
            naming: None,
            focus: cx.focus_handle(),
            error: "".into(),
            autosave: None,
            encrypted_ws,
            saving: false,
            _naming_sub: None,
        };
        this.show(selected, window, cx);
        this
    }

    pub fn current(&self) -> &Environment {
        &self.envs[self.selected]
    }

    pub fn is_global(&self) -> bool {
        self.current().kind == EnvironmentKind::Global
    }

    /// Save the env being left, then show env `ix`.
    pub fn select(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.flush(cx);
        self.show(ix, window, cx);
    }

    /// Load env `ix` into the rows. No save: when the rows don't belong to the
    /// current env (opening, after a delete) saving would wipe its variables.
    fn show(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = ix.min(self.envs.len().saturating_sub(1));
        self.naming = None;
        self.error = "".into();
        let vars = self.envs[self.selected].variables.clone();
        self.rows = vars.iter().map(|v| self.row(v, window, cx)).collect();
        self.push_blank(window, cx);
        cx.notify();
    }

    fn row(&self, v: &EnvironmentVariable, window: &mut Window, cx: &mut Context<Self>) -> VarRow {
        let key = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder("NOME");
            s.set_value(v.key.clone(), window, cx);
            s
        });
        let value = cx.new(|cx| {
            let mut s = InputState::new(window, cx)
                .placeholder("valor")
                .masked(v.encrypted);
            s.set_value(v.value.clone(), window, cx);
            s
        });
        let on_change = |this: &mut Self,
                         input: &Entity<InputState>,
                         ev: &InputEvent,
                         window: &mut Window,
                         cx: &mut Context<Self>| {
            if matches!(ev, InputEvent::Change) {
                this.edited(input, window, cx);
            }
        };
        let _subs = [
            cx.subscribe_in(&key, window, on_change),
            cx.subscribe_in(&value, window, on_change),
        ];
        VarRow {
            key,
            value,
            secret: v.encrypted,
            enabled: v.enabled,
            _subs,
        }
    }

    fn push_blank(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let blank = EnvironmentVariable {
            key: String::new(),
            value: String::new(),
            encrypted: false,
            enabled: true,
        };
        let row = self.row(&blank, window, cx);
        self.rows.push(row);
    }

    /// Typing in the blank row makes it real and opens a new blank one.
    fn edited(&mut self, input: &Entity<InputState>, window: &mut Window, cx: &mut Context<Self>) {
        let last = self.rows.last();
        if last.is_some_and(|r| r.key == *input || r.value == *input) {
            self.push_blank(window, cx);
        }
        self.changed(cx);
    }

    /// The selected env as edited (blank rows dropped).
    pub fn draft(&self, cx: &App) -> Environment {
        let mut env = self.current().clone();
        env.variables = self
            .rows
            .iter()
            .map(|r| EnvironmentVariable {
                key: r.key.read(cx).value().trim().to_string(),
                value: r.value.read(cx).value().to_string(),
                encrypted: r.secret,
                enabled: r.enabled,
            })
            .filter(|v| !v.key.is_empty() || !v.value.is_empty())
            .collect();
        env
    }

    /// Schedule the write (the next edit pushes it back).
    pub fn changed(&mut self, cx: &mut Context<Self>) {
        self.autosave = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTOSAVE_DELAY).await;
            _ = this.update(cx, |s, cx| s.flush(cx));
        }));
        cx.notify();
    }

    /// Write now if anything changed (one write at a time).
    pub fn flush(&mut self, cx: &mut Context<Self>) {
        if self.envs.is_empty() || self.saving {
            return;
        }
        let env = self.draft(cx);
        if env == *self.current() {
            return;
        }
        self.autosave = None;
        self.saving = true;
        let backend = self.backend.clone();
        let task = cx.background_spawn({
            let env = env.clone();
            async move { backend.save_env(&env) }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |s, cx| {
                s.saving = false;
                match result {
                    Ok(()) => {
                        if let Some(e) = s.envs.iter_mut().find(|e| e.id == env.id) {
                            *e = env;
                        }
                        s.emit(None, cx);
                        // Edits typed during the write.
                        s.changed(cx);
                    }
                    Err(e) => s.error = format!("falha ao salvar: {e}").into(),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn emit(&self, activate: Option<String>, cx: &mut Context<Self>) {
        cx.emit(EnvsChanged {
            envs: self.envs.clone(),
            activate,
        });
    }

    /// Synchronous write for quit, where a background save would die with the app.
    pub fn flush_blocking(&self, cx: &App) {
        if self.envs.is_empty() {
            return;
        }
        let env = self.draft(cx);
        if env != *self.current() {
            if let Err(e) = self.backend.save_env(&env) {
                eprintln!("[yuki] failed to save env {}: {e}", env.id);
            }
        }
    }

    pub fn toggle_secret(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.rows.get_mut(ix) else {
            return;
        };
        r.secret = !r.secret;
        let masked = r.secret;
        r.value.update(cx, |s, cx| s.set_masked(masked, window, cx));
        self.changed(cx);
    }

    pub fn toggle_enabled(&mut self, ix: usize, on: bool, cx: &mut Context<Self>) {
        if let Some(r) = self.rows.get_mut(ix) {
            r.enabled = on;
            self.changed(cx);
        }
    }

    pub fn remove(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix + 1 < self.rows.len() {
            self.rows.remove(ix);
            self.changed(cx);
        }
    }

    /// Start a variable named `key` (from "faltando") and put the cursor in its value.
    pub fn add_var(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let existing = self
            .rows
            .iter()
            .position(|r| r.key.read(cx).value().trim() == key);
        let ix = match existing {
            Some(ix) => ix,
            None => {
                let ix = self.rows.len() - 1;
                let blank = &self.rows[ix];
                let k = key.to_string();
                blank.key.update(cx, |s, cx| s.set_value(k, window, cx));
                self.push_blank(window, cx);
                ix
            }
        };
        // Next frame: a screen opened just now hasn't drawn this input yet.
        let focus = self.rows[ix].value.read(cx).focus_handle(cx);
        window.defer(cx, move |window, cx| focus.focus(window, cx));
        self.changed(cx);
    }

    pub fn toggle_shared(&mut self, cx: &mut Context<Self>) {
        let env = &mut self.envs[self.selected];
        let mut next = env.clone();
        next.shared = !next.shared;
        self.write_meta(next, cx);
    }

    /// Save a name/shared change straight away (not part of the row draft).
    fn write_meta(&mut self, mut next: Environment, cx: &mut Context<Self>) {
        next.variables = self.draft(cx).variables;
        let backend = self.backend.clone();
        let task = cx.background_spawn({
            let next = next.clone();
            async move { backend.save_env(&next) }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |s, cx| {
                match result {
                    Ok(()) => {
                        if let Some(e) = s.envs.iter_mut().find(|e| e.id == next.id) {
                            *e = next;
                        }
                        s.emit(None, cx);
                    }
                    Err(e) => s.error = format!("falhou: {e}").into(),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn start_naming(&mut self, kind: Naming, window: &mut Window, cx: &mut Context<Self>) {
        let name = match kind {
            Naming::New => String::new(),
            Naming::Rename => self.current().name.clone(),
        };
        let input = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder("nome do ambiente ↵");
            s.set_value(name, window, cx);
            s.select_all(window, cx);
            s
        });
        let focus = input.read(cx).focus_handle(cx);
        focus.focus(window, cx);
        self._naming_sub = Some(cx.subscribe_in(
            &input,
            window,
            |this, input, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { .. } => {
                    let name = input.read(cx).value().trim().to_string();
                    this.commit_naming(name, window, cx);
                }
                InputEvent::Blur => this.cancel_naming(cx),
                _ => {}
            },
        ));
        self.naming = Some((kind, input));
        cx.notify();
    }

    pub fn cancel_naming(&mut self, cx: &mut Context<Self>) {
        if self.naming.take().is_some() {
            cx.notify();
        }
    }

    fn commit_naming(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some((kind, _)) = self.naming.take() else {
            return;
        };
        if name.is_empty() {
            cx.notify();
            return;
        }
        match kind {
            Naming::Rename => {
                let mut next = self.current().clone();
                next.name = name;
                self.write_meta(next, cx);
            }
            Naming::New => self.create(name, window, cx),
        }
        self.focus.focus(window, cx);
    }

    fn create(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.flush(cx);
        let (backend, ws) = (self.backend.clone(), self.ws_id.clone());
        let task = cx.background_spawn(async move {
            backend
                .api
                .environments
                .create_personal(ws, name, String::new(), false)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            _ = this.update_in(cx, |s, window, cx| match result {
                Ok(env) => {
                    let id = env.id.clone();
                    s.envs.push(env);
                    s.select(s.envs.len() - 1, window, cx);
                    // A new environment is made to be used: activate it.
                    s.emit(Some(id), cx);
                    let focus = s.rows[0].key.read(cx).focus_handle(cx);
                    focus.focus(window, cx);
                }
                Err(e) => {
                    s.error = format!("falhou: {e}").into();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Delete the selected personal env (the caller confirms first).
    pub fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_global() {
            return;
        }
        self.autosave = None;
        let id = self.current().id.clone();
        let (backend, ws) = (self.backend.clone(), self.ws_id.clone());
        let task = cx.background_spawn({
            let id = id.clone();
            async move { backend.api.environments.delete(&ws, &id) }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            _ = this.update_in(cx, |s, window, cx| match result {
                Ok(()) => {
                    s.envs.retain(|e| e.id != id);
                    s.show(0, window, cx);
                    s.emit(None, cx);
                }
                Err(e) => {
                    s.error = format!("falhou: {e}").into();
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
