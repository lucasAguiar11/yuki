//! Workspaces in a folder you pick (a git repo, a project subfolder):
//! `workspaces/{id}` in the data dir becomes a symlink to it. The stores and
//! the git panel already follow the link (Voleeo's sync-folder model).

use std::{
    fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    process::Command,
};

use gpui_kit::*;
use yuki_core::{Workspace, YukiError};

use crate::{backend::Backend, prefs::Prefs, settings::SettingsScreen, workspace::Yuki};

fn io(e: impl std::fmt::Display) -> YukiError {
    YukiError::Storage(e.to_string())
}

impl Backend {
    /// The workspace already in `dir` (`workspace.yaml`, e.g. a clone), or a
    /// new one named after the folder.
    pub fn open_folder(&self, dir: &Path) -> Result<Workspace, YukiError> {
        let dir = dir.canonicalize().map_err(io)?;
        let yaml = dir.join("workspace.yaml");
        if yaml.is_file() {
            let ws: Workspace =
                serde_yaml::from_str(&fs::read_to_string(&yaml).map_err(io)?).map_err(io)?;
            let link = self.api.app_data_dir.join("workspaces").join(&ws.id);
            match link.canonicalize() {
                Ok(at) if at == dir => {}
                // Same id elsewhere: a second clone, or a copy of a local one.
                Ok(at) if at.join("workspace.yaml").is_file() => {
                    return Err(io(t!(
                        "workspace “%{name}” is already open from %{path}",
                        name = ws.name,
                        path = at.display()
                    )))
                }
                // A link left dangling or pointing at a folder the workspace
                // was moved out of gets re-pointed.
                _ => {
                    if link.is_symlink() {
                        fs::remove_file(&link).map_err(io)?;
                    }
                    symlink(&dir, &link).map_err(io)?
                }
            }
            return Ok(ws);
        }
        let name = dir
            .file_name()
            .map_or("workspace".into(), |n| n.to_string_lossy().into_owned());
        // The store writes into the data dir; move its one file out and link.
        let ws = self.api.workspaces.create(name, false)?;
        let link = self.api.app_data_dir.join("workspaces").join(&ws.id);
        fs::copy(link.join("workspace.yaml"), &yaml).map_err(io)?;
        fs::remove_dir_all(&link).map_err(io)?;
        symlink(&dir, &link).map_err(io)?;
        Ok(ws)
    }

    /// Move a workspace's files (its `.git` too) into `to` and point the link
    /// there. `to` must be empty, or already hold this workspace (moved by
    /// hand, or cloned): then only the link changes.
    pub fn move_folder(&self, id: &str, to: &Path) -> Result<PathBuf, YukiError> {
        let link = self.api.app_data_dir.join("workspaces").join(id);
        let from = self.ws_dir(id);
        let to = to.canonicalize().map_err(io)?;
        if to == from {
            return Ok(to);
        }
        if to.starts_with(&from) {
            return Err(io(t!("a folder can't go inside itself")));
        }
        if to.join("workspace.yaml").is_file() {
            // A real dir in the data dir would be orphaned (not deleted) by
            // re-linking; only a link can just be re-pointed.
            if !holds(&to, id) || !link.is_symlink() {
                return Err(io(t!("pick an empty folder")));
            }
        } else {
            if busy(&to) {
                return Err(io(t!("pick an empty folder")));
            }
            // One `mv` of the whole dir: a rename on the same volume, a copy
            // then delete across volumes.
            // ponytail: a save landing mid cross-volume copy can be lost; pause autosave if that bites.
            let _ = fs::remove_file(to.join(".DS_Store"));
            fs::remove_dir(&to).map_err(io)?;
            let out = Command::new("mv")
                .arg(&from)
                .arg(&to)
                .output()
                .map_err(io)?;
            if !out.status.success() {
                let _ = fs::create_dir(&to);
                return Err(io(String::from_utf8_lossy(&out.stderr).trim()));
            }
        }
        if link.is_symlink() {
            fs::remove_file(&link).map_err(io)?;
        }
        symlink(&to, &link).map_err(io)?;
        Ok(to)
    }

    /// `ws` into its own folder under `base`: its name, or `name-id` when
    /// another workspace's files already sit there.
    pub fn move_into(&self, ws: &Workspace, base: &Path) -> Result<PathBuf, YukiError> {
        let name = match ws.name.trim().replace(['/', ':'], "-") {
            n if n.is_empty() || n.starts_with('.') => ws.id.clone(),
            n => n,
        };
        let mut to = base.join(&name);
        if busy(&to) && !holds(&to, &ws.id) {
            to = base.join(format!("{name}-{}", ws.id));
        }
        fs::create_dir_all(&to).map_err(io)?;
        self.move_folder(&ws.id, &to)
    }

    /// Every workspace under `base`; stops at the first that fails (the ones
    /// before it stay moved — their links already point there).
    pub fn move_all(&self, base: &Path) -> Result<(), YukiError> {
        for ws in self.workspaces() {
            self.move_into(&ws, base)?;
        }
        Ok(())
    }
}

/// Anything in `dir` but Finder's `.DS_Store`.
fn busy(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|d| d.flatten().any(|e| e.file_name() != ".DS_Store"))
}

/// `dir` holds workspace `id` (its `workspace.yaml`).
fn holds(dir: &Path, id: &str) -> bool {
    fs::read_to_string(dir.join("workspace.yaml"))
        .ok()
        .and_then(|t| serde_yaml::from_str::<Workspace>(&t).ok())
        .is_some_and(|ws| ws.id == id)
}

impl Yuki {
    pub(crate) fn open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t!("open as workspace").into_owned().into()),
        });
        let backend = self.backend.clone();
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            let result = cx
                .background_spawn(async move { backend.open_folder(&dir) })
                .await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(ws) => {
                    if !this.workspaces.iter().any(|w| w.id == ws.id) {
                        this.workspaces.push(ws.clone());
                    }
                    this.open_workspace(ws, window, cx);
                }
                Err(e) => {
                    this.status = t!("couldn't open the folder: %{e}", e = e)
                        .into_owned()
                        .into();
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

impl SettingsScreen {
    pub(crate) fn move_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t!("move here").into_owned().into()),
        });
        let (backend, id) = (self.backend.clone(), self.ws.id.clone());
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            let result = cx
                .background_spawn(async move { backend.move_folder(&id, &dir) })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(folder) => {
                        this.folder = folder;
                        this.error = "".into();
                    }
                    Err(e) => {
                        this.error = t!("couldn't move the workspace: %{e}", e = e)
                            .into_owned()
                            .into()
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Pick the folder every workspace lives in: all move there now, new
    /// ones are created there (`Prefs::workspaces_dir`).
    pub(crate) fn move_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(t!("move here").into_owned().into()),
        });
        let (backend, id) = (self.backend.clone(), self.ws.id.clone());
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            let result = cx
                .background_spawn(async move {
                    backend.move_all(&dir)?;
                    Ok::<_, YukiError>((dir, backend.ws_dir(&id)))
                })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok((dir, folder)) => {
                        this.folder = folder;
                        this.error = "".into();
                        let prefs = cx.global_mut::<Prefs>();
                        prefs.workspaces_dir = Some(dir);
                        let prefs = prefs.clone();
                        this.save_prefs(prefs, cx);
                    }
                    Err(e) => {
                        this.error = t!("couldn't move the workspaces: %{e}", e = e)
                            .into_owned()
                            .into()
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
