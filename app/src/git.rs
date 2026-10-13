//! Git per workspace: each `workspaces/{id}` dir is its own repo (the
//! Voleeo model). All the git work is `yuki_git` (libgit2) off the UI thread;
//! this file holds the panel state and turns its buttons into ops.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use yuki_core::{GitBranch, GitChange, GitCommit, GitFileChange, GitRepoInfo, YukiError};

use crate::{backend::Backend, workspace::Yuki};

/// What the panel shows; reloaded whole after every op (it's a few files).
pub struct GitView {
    pub info: GitRepoInfo,
    /// One entry per changed file.
    pub changes: Vec<GitFileChange>,
    pub branches: Vec<GitBranch>,
    pub log: Vec<GitCommit>,
    pub conflicts: Vec<Conflict>,
}

/// A file left conflicted by a pull: both sides as raw YAML (`None` = deleted).
pub struct Conflict {
    pub path: String,
    pub node_id: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
}

pub struct GitInputs {
    pub message: Entity<InputState>,
    pub branch: Entity<InputState>,
    pub remote: Entity<InputState>,
    pub name: Entity<InputState>,
    pub email: Entity<InputState>,
}

#[derive(Default)]
pub struct Git {
    pub view: Option<GitView>,
    pub inputs: Option<GitInputs>,
    /// Op in flight (`"committing…"`); buttons wait.
    pub busy: Option<&'static str>,
    pub error: SharedString,
    refresh: Option<Task<()>>,
    /// The commit message box has text: the commit button is enabled.
    pub has_message: bool,
    /// Input subscriptions; outlive workspace switches (which clear `Yuki::_subs`).
    subs: Vec<Subscription>,
}

impl Git {
    /// How `node_id` changed since the last commit, for the sidebar marks.
    pub fn change_of(&self, node_id: &str) -> Option<GitChange> {
        let view = self.view.as_ref()?;
        view.changes
            .iter()
            .find(|c| c.node_id.as_deref() == Some(node_id))
            .map(|c| c.change)
    }
}

const GITIGNORE: &str = "# Yuki — machine-local files, do not commit\n.DS_Store\n";

impl Backend {
    /// The repo dir of a workspace, symlinks resolved (a workspace may live in
    /// a synced folder).
    pub fn ws_dir(&self, ws_id: &str) -> PathBuf {
        let dir = self.api.app_data_dir.join("workspaces").join(ws_id);
        std::fs::canonicalize(&dir).unwrap_or(dir)
    }
}

pub fn load_view(dir: &Path) -> Result<GitView, YukiError> {
    let mut info = yuki_git::repo_info(dir)?;
    // Before the first commit HEAD points at a branch that doesn't exist yet;
    // libgit2 then reports none. Read the name from HEAD itself.
    if info.is_repo && info.branch.is_none() {
        info.branch = std::fs::read_to_string(dir.join(".git/HEAD"))
            .ok()
            .and_then(|h| Some(h.trim().strip_prefix("ref: refs/heads/")?.to_string()));
    }
    if !info.is_repo {
        return Ok(GitView {
            info,
            changes: vec![],
            branches: vec![],
            log: vec![],
            conflicts: vec![],
        });
    }
    let mut changes = yuki_git::status(dir)?.files;
    // A file both staged and edited again shows twice (index + worktree).
    changes.dedup_by(|a, b| a.path == b.path);
    let conflicts = if info.merging {
        yuki_git::conflict_blobs(dir)?
            .into_iter()
            .map(|c| Conflict {
                path: c.path,
                node_id: c.node_id,
                ours: c.ours,
                theirs: c.theirs,
            })
            .collect()
    } else {
        vec![]
    };
    Ok(GitView {
        branches: yuki_git::branches(dir).unwrap_or_default(),
        log: yuki_git::log(dir, 30).unwrap_or_default(),
        info,
        changes,
        conflicts,
    })
}

/// Start versioning a workspace, with the app's `.gitignore`.
pub fn init(dir: &Path) -> Result<Option<String>, YukiError> {
    yuki_git::init(dir)?;
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(ignore, GITIGNORE).map_err(|e| YukiError::Git(e.to_string()))?;
    }
    Ok(Some(t!("repository created").into_owned()))
}

pub(crate) type GitOp = Box<dyn FnOnce(&Path) -> Result<Option<String>, YukiError> + Send>;

impl Yuki {
    pub(crate) fn init_git_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut input = |placeholder: &'static str, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(t!(placeholder).into_owned()))
        };
        let inputs = GitInputs {
            message: input("commit message ↵", cx),
            branch: input("new branch ↵", cx),
            remote: input("remote url (origin) ↵", cx),
            name: input("your name", cx),
            email: input("your e-mail ↵", cx),
        };
        let on_enter =
            |cx: &mut Context<Self>,
             window: &mut Window,
             input: &Entity<InputState>,
             f: fn(&mut Yuki, String, &mut Window, &mut Context<Yuki>)| {
                cx.subscribe_in(
                    input,
                    window,
                    move |this, input, ev: &InputEvent, window, cx| {
                        if matches!(ev, InputEvent::PressEnter { .. }) {
                            let text = input.read(cx).value().trim().to_string();
                            if !text.is_empty() {
                                f(this, text, window, cx);
                            }
                        }
                    },
                )
            };
        let subs = [
            on_enter(cx, window, &inputs.message, Self::git_commit),
            on_enter(cx, window, &inputs.branch, Self::git_new_branch),
            on_enter(cx, window, &inputs.remote, Self::git_set_remote),
            on_enter(cx, window, &inputs.email, |this, _, window, cx| {
                this.git_set_identity(window, cx)
            }),
        ];
        self.git.subs.extend(subs);
        // Re-render only when the box flips between empty and not (button state).
        self.git.subs.push(cx.subscribe_in(
            &inputs.message,
            window,
            |this, input, ev: &InputEvent, _, cx| {
                let has = !input.read(cx).value().trim().is_empty();
                if matches!(ev, InputEvent::Change) && has != this.git.has_message {
                    this.git.has_message = has;
                    cx.notify();
                }
            },
        ));
        self.git.inputs = Some(inputs);
    }

    /// Re-read repo state shortly after the last change (autosave bursts).
    pub(crate) fn refresh_git(&mut self, cx: &mut Context<Self>) {
        let Some(ws) = self.ws.as_ref().map(|w| w.id.clone()) else {
            return;
        };
        let backend = self.backend.clone();
        self.git.refresh = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            let view = cx
                .background_spawn(async move { load_view(&backend.ws_dir(&ws)) })
                .await;
            _ = this.update(cx, |this, cx| {
                if let Ok(view) = view {
                    this.git.view = Some(view);
                    cx.notify();
                }
            });
        }));
    }

    /// Flush open edits, run `op` on the repo, then reload the panel — and the
    /// tree + tabs when `reload` (the op rewrote request files).
    pub(crate) fn git_op(
        &mut self,
        busy: &'static str,
        reload: bool,
        op: GitOp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ws) = self.ws.as_ref().map(|w| w.id.clone()) else {
            return;
        };
        if self.git.busy.is_some() {
            return;
        }
        let drafts: Vec<_> = self
            .tabs
            .iter()
            .filter(|t| t.read(cx).dirty)
            .map(|t| t.read(cx).draft(cx))
            .collect();
        self.git.busy = Some(busy);
        self.git.error = "".into();
        cx.notify();
        let backend = self.backend.clone();
        let task = cx.background_spawn(async move {
            for d in &drafts {
                backend.save(d)?;
            }
            let dir = backend.ws_dir(&ws);
            let result = op(&dir);
            let view = load_view(&dir)?;
            let data = if reload {
                Some(backend.load(&ws)?)
            } else {
                None
            };
            Ok::<_, YukiError>((result, view, data))
        });
        cx.spawn_in(window, async move |this, cx| {
            let out = task.await;
            this.update_in(cx, |this, window, cx| {
                this.git.busy = None;
                match out {
                    Ok((result, view, data)) => {
                        this.git.view = Some(view);
                        match result {
                            Ok(msg) => this.status = msg.unwrap_or_default().into(),
                            Err(e) => this.git.error = e.to_string().into(),
                        }
                        if let Some(data) = data {
                            this.reopen_tabs(data, window, cx);
                        }
                    }
                    Err(e) => this.git.error = e.to_string().into(),
                }
                cx.notify();
            })
        })
        .detach();
    }

    /// Files changed under the editors (checkout, pull, conflict pick): rebuild
    /// every open tab from disk, keeping which one was active.
    fn reopen_tabs(
        &mut self,
        data: crate::backend::WorkspaceData,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ids: Vec<String> = self
            .tabs
            .iter()
            .map(|t| t.read(cx).saved.id.clone())
            .collect();
        let active = ids.get(self.active).cloned();
        self.tabs.clear();
        self.data = Some(data);
        self.refresh_rows();
        for id in &ids {
            self.open_request(id, window, cx);
        }
        let ix = active.and_then(|a| self.tabs.iter().position(|t| t.read(cx).saved.id == a));
        self.active = ix.unwrap_or(0);
        self.focus_active(window, cx);
    }

    /// `GET list posts` / `folder auth` / the file name, for a changed path.
    pub fn git_label(&self, node_id: Option<&str>, path: &str) -> String {
        let data = self.data.as_ref();
        let id = node_id.unwrap_or("");
        if let Some(r) = data.and_then(|d| d.requests.iter().find(|r| r.id == id)) {
            let request = format!("{} {}", r.method, r.name);
            return if path.ends_with(".md") {
                t!("%{request} · notes", request = request).into_owned()
            } else {
                request
            };
        }
        if let Some(f) = data.and_then(|d| d.folders.iter().find(|f| f.id == id)) {
            return t!("folder %{name}", name = f.name).into_owned();
        }
        match path {
            "workspace.yaml" => "workspace".into(),
            p => p.to_string(),
        }
    }
}

/// `5 min ago` / `3 h ago` / `12/09` for a unix timestamp.
pub fn ago(ts: f64) -> String {
    let secs = chrono::Utc::now().timestamp() - ts as i64;
    match secs {
        ..60 => t!("now").into_owned(),
        60..3600 => t!("%{n} min ago", n = secs / 60).into_owned(),
        3600..86400 => t!("%{n} h ago", n = secs / 3600).into_owned(),
        _ => chrono::DateTime::from_timestamp(ts as i64, 0)
            .map(|t| t.with_timezone(&chrono::Local).format("%d/%m").to_string())
            .unwrap_or_default(),
    }
}
