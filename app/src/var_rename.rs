//! Rename a variable across the workspace (vars panel → right-click): every
//! env and folder that defines it, and each `{{ NAME }}` in requests,
//! folder/workspace headers and auth, and variable values.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use yuki_core::{EnvironmentVariable, HttpRequest, YukiError};
use yuki_mcp::{api::ApiBackend, resolve::is_identifier};

use crate::{secrets, vars, workspace::Yuki};

pub struct VarRename {
    pub name: String,
    pub input: Entity<InputState>,
    _sub: Subscription,
}

impl Yuki {
    pub fn start_var_rename(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_value(name.clone(), window, cx);
            s.select_all(window, cx);
            s
        });
        // Deferred: the menu closing would pull focus back out.
        let focus = input.read(cx).focus_handle(cx);
        window.defer(cx, move |window, cx| focus.focus(window, cx));
        let _sub = cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                this.commit_var_rename(window, cx);
            }
        });
        self.var_renaming = Some(VarRename { name, input, _sub });
        cx.notify();
    }

    pub fn cancel_var_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.var_renaming.take().is_some() {
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    fn commit_var_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.var_renaming.take() else {
            return;
        };
        let (old, new) = (r.name, r.input.read(cx).value().trim().to_string());
        self.focus_active(window, cx);
        cx.notify();
        let Some(ws) = self.ws.as_ref().map(|w| w.id.clone()) else {
            return;
        };
        if new.is_empty() || new == old {
            return;
        }
        if !is_identifier(&new) {
            self.status = t!("%{name} isn't a valid variable name", name = new)
                .into_owned()
                .into();
            return;
        }
        // Everything open lands on disk first, then leaves: the rename rewrites
        // those files and the screens would save their old copies back.
        self.flush_envs_blocking(cx);
        if let Some((screen, _)) = self.env_screen.take() {
            drop(screen);
        }
        if let Some((view, _)) = self.scope.take() {
            view.read(cx).flush_blocking(cx);
            self.scope_active = false;
        }
        let drafts: Vec<HttpRequest> = (self.tabs.iter())
            .filter(|t| t.read(cx).dirty)
            .map(|t| t.read(cx).draft(cx))
            .collect();
        let backend = self.backend.clone();
        let task = cx.background_spawn({
            let (old, new) = (old.clone(), new.clone());
            async move {
                for d in &drafts {
                    backend.save(d)?;
                }
                let done = rename(&backend.api, &ws, &old, &new)?;
                Ok::<_, YukiError>((done, backend.load(&ws)?))
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok((false, _)) => {
                        this.status = t!("%{name} already exists", name = new).into_owned().into()
                    }
                    Ok((true, data)) => {
                        this.set_envs(&data.environments, None, window, cx);
                        // A rewritten request reopens from disk (its draft was
                        // saved above); the rest only re-read their variables.
                        for tab in this.tabs.clone() {
                            let id = tab.read(cx).saved.id.clone();
                            let disk = data.requests.iter().find(|r| r.id == id).cloned();
                            tab.update(cx, |e, cx| match disk {
                                Some(d) if d.updated_at != e.saved.updated_at || e.dirty => {
                                    e.rebuild(d, window, cx)
                                }
                                _ => e.load_vars(cx),
                            });
                        }
                        this.data = Some(data);
                        this.refresh_rows();
                        this.refresh_git(cx);
                        this.focus_active(window, cx);
                    }
                    Err(e) => this.status = t!("failed: %{e}", e = e).to_string().into(),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// `text` with each `{{ old }}` naming `new`, spacing kept; `None` if it has none.
pub fn rename_in(text: &str, old: &str, new: &str) -> Option<String> {
    let mut out = String::new();
    let mut last = 0;
    for (range, expr) in vars::spans(text) {
        if expr != old {
            continue;
        }
        let inner = &text[range.start + 2..range.end - 2];
        let at = range.start + 2 + inner.len() - inner.trim_start().len();
        out.push_str(&text[last..at]);
        out.push_str(new);
        last = at + old.len();
    }
    (last > 0).then(|| out + &text[last..])
}

fn walk(v: &mut Value, old: &str, new: &str) -> bool {
    match v {
        Value::String(s) => rename_in(s, old, new).map(|n| *s = n).is_some(),
        Value::Array(a) => a.iter_mut().fold(false, |hit, v| walk(v, old, new) | hit),
        Value::Object(o) => o.values_mut().fold(false, |hit, v| walk(v, old, new) | hit),
        _ => false,
    }
}

/// `item` with its templates renamed; `None` if it has none.
fn renamed<T: Serialize + DeserializeOwned>(
    item: &T,
    old: &str,
    new: &str,
) -> Result<Option<T>, YukiError> {
    let err = |e: serde_json::Error| YukiError::Storage(e.to_string());
    let mut v = serde_json::to_value(item).map_err(err)?;
    if !walk(&mut v, old, new) {
        return Ok(None);
    }
    serde_json::from_value(v).map(Some).map_err(err)
}

/// Renames the definitions and the templates in the values; true if any changed.
fn rename_vars(vars: &mut [EnvironmentVariable], old: &str, new: &str) -> bool {
    vars.iter_mut().fold(false, |hit, v| {
        let key = v.key == old;
        if key {
            v.key = new.into();
        }
        rename_in(&v.value, old, new).map(|s| v.value = s).is_some() | key | hit
    })
}

/// Rewrites every file that names `old`; `false` (nothing written) when an env
/// or folder already defines `new`. Secrets are opened to rename inside them
/// and sealed back (an untouched one keeps its ciphertext).
fn rename(api: &ApiBackend, ws: &str, old: &str, new: &str) -> Result<bool, YukiError> {
    let key = secrets::key(api, ws)?;
    let envs = api.environments.list(ws)?;
    let folders = api.requests.list_folders(ws)?;
    let taken = |vars: &[EnvironmentVariable]| vars.iter().any(|v| v.key == new);
    if envs.iter().any(|e| taken(&e.variables)) || folders.iter().any(|f| taken(&f.variables)) {
        return Ok(false);
    }

    for stored in envs {
        let mut env = stored.clone();
        if let Some(k) = &key {
            secrets::open_vars(&mut env.variables, k);
        }
        if rename_vars(&mut env.variables, old, new) {
            if let Some(k) = &key {
                secrets::seal_vars(&mut env.variables, &stored.variables, k)?;
            }
            api.environments.save(&env)?;
        }
    }

    for stored in folders {
        let mut f = stored.clone();
        if let Some(k) = &key {
            f.auth = secrets::open(f.auth, k);
            secrets::open_vars(&mut f.variables, k);
        }
        let headers = renamed(&f.headers, old, new)?;
        let auth = renamed(&f.auth, old, new)?;
        if headers.is_some() || auth.is_some() {
            let mut auth = auth.unwrap_or(f.auth);
            if let Some(k) = &key {
                auth = secrets::seal(&auth, &stored.auth, k)?;
            }
            let headers = headers.unwrap_or(f.headers);
            api.requests.update_folder(ws, &f.id, headers, auth)?;
        }
        if rename_vars(&mut f.variables, old, new) {
            if let Some(k) = &key {
                secrets::seal_vars(&mut f.variables, &stored.variables, k)?;
            }
            api.requests
                .update_folder_variables(ws, &f.id, f.variables)?;
        }
    }

    let stored = api.workspaces.get(ws)?;
    let mut w = stored.clone();
    if let Some(k) = &key {
        w.auth = secrets::open(w.auth, k);
    }
    if let Some(mut w) = renamed(&w, old, new)? {
        if let Some(k) = &key {
            w.auth = secrets::seal(&w.auth, &stored.auth, k)?;
        }
        api.workspaces.save(&w)?;
    }

    for stored in api.requests.list_requests(ws)? {
        let mut req = stored.clone();
        if let Some(k) = &key {
            for auth in req.auths_mut() {
                *auth = secrets::open(std::mem::take(auth), k);
            }
        }
        if let Some(mut req) = renamed(&req, old, new)? {
            if let Some(k) = &key {
                secrets::seal_request(&mut req, &stored, k)?;
            }
            api.requests.put_request(&req)?;
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::rename_in;

    #[test]
    fn renames_only_that_variable_keeping_spacing() {
        let r = |t| rename_in(t, "TOKEN", "API_KEY");
        assert_eq!(
            r("Bearer {{TOKEN}} {{ TOKEN }} {{ TOKEN_2 }} {{ $fake.email() }}").as_deref(),
            Some("Bearer {{API_KEY}} {{ API_KEY }} {{ TOKEN_2 }} {{ $fake.email() }}")
        );
        assert_eq!(r("{{ TOKENS }} TOKEN {{BASE}}"), None);
    }
}
