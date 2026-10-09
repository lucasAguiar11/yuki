//! "Configurações" screen (⌘,): the workspace (name, DNS overrides, shell env
//! allowlist) and the app's appearance (font sizes, language). Workspace edits
//! go to disk 400 ms after typing stops; appearance applies and saves on click.

use std::{sync::Arc, time::Duration};

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::*;
use yuki_core::{Workspace, YukiError};
use yuki_mcp::resolve;

use crate::{
    backend::Backend,
    draft,
    prefs::{Font, Prefs},
    workspace::Yuki,
};

const AUTOSAVE_DELAY: Duration = Duration::from_millis(400);

#[derive(Clone, Copy, PartialEq)]
pub enum Section {
    Workspace,
    Appearance,
}

/// A workspace write landed: the app refreshes its copies (title, palette).
pub struct WorkspaceSaved(pub Workspace);

pub struct SettingsScreen {
    backend: Arc<Backend>,
    /// As last written.
    ws: Workspace,
    allow: Vec<String>,
    pub section: Section,
    pub name: Entity<InputState>,
    pub dns: Entity<TextareaState>,
    pub env: Entity<InputState>,
    pub focus: FocusHandle,
    pub error: SharedString,
    autosave: Option<Task<()>>,
    saving: bool,
    _subs: [Subscription; 3],
}

impl EventEmitter<WorkspaceSaved> for SettingsScreen {}

impl Backend {
    /// Name/DNS go onto the workspace as stored now (a pull may have changed
    /// other fields); writes only what differs.
    fn save_ws_settings(
        &self,
        ws: &Workspace,
        allow: Option<&[String]>,
    ) -> Result<Workspace, YukiError> {
        let store = &self.api.workspaces;
        let mut stored = store.get(&ws.id)?;
        if stored.name != ws.name || stored.dns_overrides != ws.dns_overrides {
            stored.name = ws.name.clone();
            stored.dns_overrides = ws.dns_overrides.clone();
            stored.updated_at = chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.6f")
                .to_string();
            store.save(&stored)?;
        }
        if let Some(names) = allow {
            resolve::set_system_env_allowlist(&ws.id, &self.api.app_data_dir, names)?;
        }
        Ok(stored)
    }
}

fn parse_names(text: &str) -> Vec<String> {
    text.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

impl SettingsScreen {
    fn new(
        backend: Arc<Backend>,
        ws: Workspace,
        allow: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).default_value(ws.name.clone()));
        let dns = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(4, 12)
                .default_value(draft::hosts_text(&ws.dns_overrides))
        });
        let env = cx.new(|cx| InputState::new(window, cx).default_value(allow.join(", ")));
        let on_change = |this: &mut Self, ev: &InputEvent, cx: &mut Context<Self>| {
            if matches!(ev, InputEvent::Change) {
                this.changed(cx);
            }
        };
        let _subs = [
            cx.subscribe(&name, move |this, _, ev, cx| on_change(this, ev, cx)),
            cx.subscribe(&dns, move |this, _, ev, cx| on_change(this, ev, cx)),
            cx.subscribe(&env, move |this, _, ev, cx| on_change(this, ev, cx)),
        ];
        Self {
            backend,
            ws,
            allow,
            section: Section::Workspace,
            name,
            dns,
            env,
            focus: cx.focus_handle(),
            error: "".into(),
            autosave: None,
            saving: false,
            _subs,
        }
    }

    pub fn select(&mut self, section: Section, cx: &mut Context<Self>) {
        self.section = section;
        cx.notify();
    }

    /// The workspace as edited, plus the allowlist when it changed. An empty
    /// name keeps the old one; untouched DNS text keeps the stored rows as-is.
    fn draft(&self, cx: &App) -> (Workspace, Option<Vec<String>>) {
        let mut ws = self.ws.clone();
        let name = self.name.read(cx).value().trim().to_string();
        if !name.is_empty() {
            ws.name = name;
        }
        let dns = self.dns.read(cx).value();
        if dns != draft::hosts_text(&ws.dns_overrides) {
            ws.dns_overrides = draft::hosts_rows(&dns, &ws.dns_overrides);
        }
        let allow = parse_names(&self.env.read(cx).value());
        (ws, (allow != self.allow).then_some(allow))
    }

    fn dirty(&self, ws: &Workspace, allow: &Option<Vec<String>>) -> bool {
        ws.name != self.ws.name || ws.dns_overrides != self.ws.dns_overrides || allow.is_some()
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.autosave = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTOSAVE_DELAY).await;
            _ = this.update(cx, |s, cx| s.flush(cx));
        }));
    }

    /// Write now if anything changed (one write at a time).
    pub fn flush(&mut self, cx: &mut Context<Self>) {
        let (ws, allow) = self.draft(cx);
        if self.saving || !self.dirty(&ws, &allow) {
            return;
        }
        self.autosave = None;
        self.saving = true;
        let backend = self.backend.clone();
        let task = cx.background_spawn({
            let allow = allow.clone();
            async move { backend.save_ws_settings(&ws, allow.as_deref()) }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |s, cx| {
                s.saving = false;
                match result {
                    Ok(stored) => {
                        s.error = "".into();
                        if let Some(a) = allow {
                            s.allow = a;
                        }
                        s.ws = stored.clone();
                        cx.emit(WorkspaceSaved(stored));
                        // Edits typed during the write.
                        s.changed(cx);
                    }
                    Err(e) => s.error = t!("save failed: %{e}", e = e).into_owned().into(),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Synchronous write for quit, where a background save would die with the app.
    pub fn flush_blocking(&self, cx: &App) {
        let (ws, allow) = self.draft(cx);
        if self.dirty(&ws, &allow) {
            if let Err(e) = self.backend.save_ws_settings(&ws, allow.as_deref()) {
                eprintln!("[yuki] failed to save workspace {}: {e}", ws.id);
            }
        }
    }

    /// Step a font size; applied at once, saved off-thread.
    pub fn bump(&mut self, font: Font, delta: f32, cx: &mut Context<Self>) {
        let prefs = Prefs::bump(font, delta, cx);
        self.save_prefs(prefs, cx);
    }

    pub fn set_language(&mut self, lang: Option<&str>, cx: &mut Context<Self>) {
        let prefs = Prefs::set_language(lang, cx);
        self.save_prefs(prefs, cx);
    }

    fn save_prefs(&mut self, prefs: Prefs, cx: &mut Context<Self>) {
        let task = cx.background_spawn(async move { prefs.save() });
        cx.spawn(async move |this, cx| {
            if let Err(e) = task.await {
                _ = this.update(cx, |s, cx| {
                    s.error = t!("save failed: %{e}", e = e).into_owned().into();
                    cx.notify();
                });
            }
        })
        .detach();
    }
}

impl Yuki {
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.is_some() {
            return;
        }
        let Some(id) = self.ws.as_ref().map(|w| w.id.clone()) else {
            return;
        };
        self.close_envs(window, cx);
        let backend = self.backend.clone();
        let task = cx.background_spawn(async move {
            let ws = backend.api.workspaces.get(&id)?;
            let allow = resolve::system_env_allowlist(&id, &backend.api.app_data_dir);
            Ok::<_, YukiError>((ws, allow))
        });
        cx.spawn_in(window, async move |this, cx| {
            let loaded = task.await;
            _ = this.update_in(cx, |this, window, cx| {
                match loaded {
                    Ok((ws, allow)) => {
                        let backend = this.backend.clone();
                        let screen =
                            cx.new(|cx| SettingsScreen::new(backend, ws, allow, window, cx));
                        let sub = cx.subscribe(&screen, |this, _, ev: &WorkspaceSaved, cx| {
                            let ws = &ev.0;
                            if let Some(w) = this.workspaces.iter_mut().find(|w| w.id == ws.id) {
                                *w = ws.clone();
                            }
                            if this.ws.as_ref().is_some_and(|w| w.id == ws.id) {
                                this.ws = Some(ws.clone());
                            }
                            cx.notify();
                        });
                        let focus = screen.read(cx).focus.clone();
                        focus.focus(window, cx);
                        this.settings = Some((screen, sub));
                    }
                    Err(e) => {
                        this.status = t!("failed to read the workspace: %{e}", e = e)
                            .into_owned()
                            .into()
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((screen, _sub)) = self.settings.take() {
            screen.update(cx, |s, cx| s.flush(cx));
            self.focus_active(window, cx);
            cx.notify();
        }
    }
}
