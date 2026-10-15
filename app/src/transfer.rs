//! Import/export between workspaces and collection files. The engine crates
//! are pure: here they meet the stores and the workspace secrets (opened for
//! export, sealed on import).

use yuki_core::{AuthConfig, Workspace, YukiError};
use yuki_export::{Bundle, NamedFile};
use yuki_import::ImportFormat;

use crate::{backend::Backend, prefs::Prefs, secrets};

/// Where an import lands.
pub enum Dest {
    New(String),
    Existing {
        ws_id: String,
        folder: Option<String>,
    },
}

pub struct Imported {
    pub workspace: Workspace,
    pub requests: usize,
    pub warnings: Vec<String>,
}

/// From the import/export screens to the window.
pub enum TransferEvent {
    Close,
    Imported(Box<Imported>),
    Exported(std::path::PathBuf, Vec<String>),
}

#[derive(Clone, Copy, PartialEq)]
pub enum ExportFormat {
    Voleeo,
    Yaak,
    Postman,
    Insomnia,
}

impl ExportFormat {
    pub const ALL: [Self; 4] = [Self::Voleeo, Self::Yaak, Self::Postman, Self::Insomnia];

    pub fn label(self) -> &'static str {
        match self {
            Self::Voleeo => "voleeo",
            Self::Yaak => "yaak",
            Self::Postman => "postman",
            Self::Insomnia => "insomnia",
        }
    }

    fn file_suffix(self) -> &'static str {
        match self {
            Self::Voleeo => ".voleeo.yaml",
            Self::Yaak => ".yaak.json",
            Self::Postman => ".postman_collection.json",
            Self::Insomnia => ".insomnia.json",
        }
    }
}

pub struct Exported {
    pub content: String,
    /// Written next to the main file (Postman environments).
    pub siblings: Vec<NamedFile>,
    pub warnings: Vec<String>,
}

fn err(e: impl std::fmt::Display) -> YukiError {
    YukiError::Storage(e.to_string())
}

impl Backend {
    pub fn import(
        &self,
        format: ImportFormat,
        content: &str,
        selected: Option<&[String]>,
        dest: &Dest,
    ) -> Result<Imported, YukiError> {
        let api = &self.api;
        let (workspace, parent) = match dest {
            Dest::New(name) => {
                let ws = api.workspaces.create(name.clone(), false)?;
                if let Some(base) = Prefs::load().workspaces_dir {
                    self.move_into(&ws, &base)?;
                }
                (ws, None)
            }
            Dest::Existing { ws_id, folder } => (api.workspaces.get(ws_id)?, folder.clone()),
        };
        let ws_id = workspace.id.clone();
        // Append after whatever already sits where the import lands.
        let siblings = api
            .requests
            .list_requests(&ws_id)?
            .into_iter()
            .map(|r| (r.folder_id, r.order));
        let order_base = siblings
            .chain(
                api.requests
                    .list_folders(&ws_id)?
                    .into_iter()
                    .map(|f| (f.folder_id, f.order)),
            )
            .filter(|(p, _)| *p == parent)
            .map(|(_, o)| o + 1.0)
            .fold(0.0, f64::max);
        let mut plan =
            yuki_import::plan(format, content, selected, &ws_id, order_base).map_err(err)?;

        if parent.is_some() {
            let roots = plan.folders.iter_mut().map(|f| &mut f.folder_id);
            let roots = roots
                .chain(plan.requests.iter_mut().map(|r| &mut r.folder_id))
                .chain(plan.websockets.iter_mut().map(|w| &mut w.folder_id))
                .chain(plan.grpc.iter_mut().map(|g| &mut g.folder_id));
            for p in roots.filter(|p| p.is_none()) {
                *p = parent.clone();
            }
        }
        if let Some(key) = secrets::key(api, &ws_id)? {
            for f in &mut plan.folders {
                secrets::seal_vars(&mut f.variables, &[], &key)?;
            }
            for r in &mut plan.requests {
                r.auth = secrets::seal(&r.auth, &AuthConfig::None, &key)?;
            }
            for w in &mut plan.websockets {
                w.auth = secrets::seal(&w.auth, &AuthConfig::None, &key)?;
            }
            for g in &mut plan.grpc {
                g.auth = secrets::seal(&g.auth, &AuthConfig::None, &key)?;
            }
        }
        api.requests.write_bulk(&plan.folders, &plan.requests)?;
        for w in &plan.websockets {
            api.ws.save(w)?;
        }
        for g in &plan.grpc {
            api.grpc.save(g)?;
        }
        // Globals join the Global env; a name already there keeps its value.
        if !plan.variables.is_empty() {
            let envs = self.environments(&ws_id)?;
            if let Some(mut global) = envs
                .into_iter()
                .find(|e| e.id == yuki_storage::GLOBAL_ENV_ID)
            {
                for v in plan.variables {
                    if !global.variables.iter().any(|g| g.key == v.key) {
                        global.variables.push(v);
                    }
                }
                self.save_env(&global)?;
            }
        }
        for env in &plan.environments {
            self.save_env(env)?;
        }
        Ok(Imported {
            workspace,
            requests: plan.requests.len(),
            warnings: plan.warnings,
        })
    }

    /// The whole workspace, secrets opened, in `format`; `secrets: false`
    /// writes every secret value empty.
    pub fn export(
        &self,
        ws_id: &str,
        format: ExportFormat,
        secrets: bool,
    ) -> Result<Exported, YukiError> {
        let api = &self.api;
        let mut workspace = api.workspaces.get(ws_id)?;
        let mut folders = api.requests.list_folders(ws_id)?;
        let mut ws = api.ws.list(ws_id)?;
        let mut grpc = api.grpc.list(ws_id)?;
        if let Some(key) = secrets::key(api, ws_id)? {
            workspace.auth = secrets::open(std::mem::take(&mut workspace.auth), &key);
            for f in &mut folders {
                f.auth = secrets::open(std::mem::take(&mut f.auth), &key);
                secrets::open_vars(&mut f.variables, &key);
            }
            for w in &mut ws {
                w.auth = secrets::open(std::mem::take(&mut w.auth), &key);
            }
            for g in &mut grpc {
                g.auth = secrets::open(std::mem::take(&mut g.auth), &key);
            }
        }
        let mut bundle = Bundle {
            requests: self.load(ws_id)?.requests,
            environments: self.environments(ws_id)?,
            workspace,
            folders,
            ws,
            grpc,
        };
        if !secrets {
            bundle.strip_secrets();
        }
        let bundles = [bundle];
        let out = match format {
            ExportFormat::Voleeo => yuki_export::to_voleeo(&bundles)?,
            ExportFormat::Yaak => yuki_export::to_yaak(&bundles)?,
            ExportFormat::Postman => yuki_export::to_postman(&bundles)?,
            ExportFormat::Insomnia => yuki_export::to_insomnia(&bundles)?,
        };
        let siblings = match format {
            ExportFormat::Postman => yuki_export::postman_environments(&bundles)?,
            _ => vec![],
        };
        Ok(Exported {
            content: out.content,
            siblings,
            warnings: out.warnings,
        })
    }
}

/// `my workspace` → `my-workspace.postman_collection.json`.
pub fn file_name(name: &str, format: ExportFormat) -> String {
    format!("{}{}", slug(name), format.file_suffix())
}

/// The main file at `path` (the save dialog already confirmed it) plus the
/// Postman environment files next to it as `<stem>.<env>.postman_environment.json`,
/// numbered rather than overwriting anything already there.
pub fn write_export(path: &std::path::Path, out: &Exported) -> Result<(), String> {
    std::fs::write(path, &out.content).map_err(|e| e.to_string())?;
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    let file = path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = file
        .strip_suffix(ExportFormat::Postman.file_suffix())
        .unwrap_or(&file);
    for env in &out.siblings {
        let base = format!("{stem}.{}", slug(&env.name));
        let free = (1..)
            .map(|n| match n {
                1 => dir.join(format!("{base}.postman_environment.json")),
                n => dir.join(format!("{base}-{n}.postman_environment.json")),
            })
            .find(|p| !p.exists())
            .expect("unbounded");
        std::fs::write(free, &env.content).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if s.is_empty() {
        "workspace".into()
    } else {
        s
    }
}
