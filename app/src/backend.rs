use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use yuki_core::{
    ApiFolder, Environment, EnvironmentKind, EnvironmentVariable, HttpRequest, HttpResponse,
    Workspace, YukiError,
};
use yuki_mcp::{api::ApiBackend, resolve};
use yuki_storage::StoredHttpResponseSummary;

use crate::vars::{Source, Var, Vars};

/// Engine handle shared by every view. Stores do blocking YAML I/O — call them
/// off the UI thread (`cx.background_spawn`); `send` runs on the tokio runtime
/// because reqwest needs one.
pub struct Backend {
    pub api: Arc<ApiBackend>,
    rt: tokio::runtime::Runtime,
}

pub struct WorkspaceData {
    pub folders: Vec<ApiFolder>,
    pub requests: Vec<HttpRequest>,
    pub environments: Vec<Environment>,
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("YUKI_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| platform_data_dir().join("dev.yuki"))
}

fn platform_data_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from("."))
}

impl Backend {
    pub fn open() -> Result<Arc<Self>, YukiError> {
        let dir = data_dir();
        fs::create_dir_all(&dir).map_err(|e| YukiError::Storage(e.to_string()))?;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| YukiError::Storage(e.to_string()))?;
        // ApiBackend::open builds the reqwest client, which wants a runtime context.
        let api = {
            let _guard = rt.enter();
            ApiBackend::open(dir, Arc::new(|_, _| {}))?
        };
        Ok(Arc::new(Self {
            api: Arc::new(api),
            rt,
        }))
    }

    pub fn workspaces(&self) -> Vec<Workspace> {
        let mut list = self.api.workspaces.list().unwrap_or_default();
        list.sort_by_key(|w| w.name.to_lowercase());
        list
    }

    pub fn load(&self, ws_id: &str) -> Result<WorkspaceData, YukiError> {
        Ok(WorkspaceData {
            folders: self.api.requests.list_folders(ws_id)?,
            requests: self.api.requests.list_requests(ws_id)?,
            environments: self.api.environments.list(ws_id)?,
        })
    }

    pub fn save(&self, req: &HttpRequest) -> Result<(), YukiError> {
        save(&self.api, req)
    }

    /// Save, then send through the shared engine pipeline (env/folder vars,
    /// auth, cookie jar, history).
    pub async fn save_and_send(
        &self,
        req: HttpRequest,
        env_id: Option<String>,
    ) -> Result<HttpResponse, String> {
        // Only `api` goes into the task: a task owning the Backend could drop
        // the runtime from inside itself.
        let api = self.api.clone();
        self.rt
            .spawn(async move {
                let (api2, to_save) = (api.clone(), req.clone());
                tokio::task::spawn_blocking(move || save(&api2, &to_save))
                    .await
                    .map_err(|e| e.to_string())?
                    .map_err(|e| e.to_string())?;
                api.send_http(req.workspace_id, req.id, env_id, None, false)
                    .await
            })
            .await
            .map_err(|e| e.to_string())?
    }

    pub fn cancel(&self, req_id: &str) {
        self.api.executor.cancel(req_id);
    }

    /// Newest-first summaries of a request's stored responses.
    pub fn history(&self, ws_id: &str, req_id: &str) -> Vec<StoredHttpResponseSummary> {
        self.api.responses.list(ws_id, req_id).unwrap_or_default()
    }

    /// A stored response with its body inlined (large bodies live in a side
    /// file), plus the resolved request that produced it, when recorded.
    pub fn stored_response(
        &self,
        ws_id: &str,
        req_id: &str,
        resp_id: &str,
    ) -> Result<(HttpResponse, Option<HttpRequest>), YukiError> {
        let responses = &self.api.responses;
        let stored = responses
            .get(ws_id, req_id, resp_id)?
            .ok_or_else(|| YukiError::NotFound(format!("response {resp_id}")))?;
        let body = responses.read_full_body(ws_id, &stored)?;
        let resp = HttpResponse {
            body,
            body_windowed: false,
            ..stored.response
        };
        Ok((resp, stored.resolved_request))
    }

    /// The request exactly as sent (literal URL/headers/body) for a stored response.
    pub fn sent_request(&self, ws_id: &str, req_id: &str, resp_id: &str) -> Option<HttpRequest> {
        self.api
            .responses
            .get(ws_id, req_id, resp_id)
            .ok()??
            .resolved_request
    }

    /// Every variable visible to a request in `folder_id` under `env_id`, resolved,
    /// with the layer that wins: nearest folder > personal env > global > system.
    pub fn variables(&self, ws_id: &str, folder_id: Option<&str>, env_id: Option<&str>) -> Vars {
        let api = &self.api;
        let envs = api.environments.list(ws_id).unwrap_or_default();
        let folders = api.requests.list_folders(ws_id).unwrap_or_default();
        let key = yuki_crypto::load_key_from_file(ws_id, &api.app_data_dir).ok();
        let mut values = resolve::load_env_vars_from(&envs, ws_id, env_id, &api.app_data_dir);
        resolve::apply_folder_vars(&mut values, folder_id, &folders, key.as_ref());

        // Nearest folder first; bounded so a corrupt parent cycle can't loop.
        let mut chain: Vec<&ApiFolder> = Vec::new();
        let mut next = folder_id;
        while let Some(f) = next.and_then(|id| folders.iter().find(|f| f.id == id)) {
            if chain.len() >= folders.len() {
                break;
            }
            chain.push(f);
            next = f.folder_id.as_deref();
        }
        let personal = env_id.and_then(|id| envs.iter().find(|e| e.id == id));
        let global = envs.iter().find(|e| e.kind == EnvironmentKind::Global);
        let defined = |vars: &[EnvironmentVariable], name: &str| {
            vars.iter()
                .find(|v| v.enabled && v.key == name)
                .map(|v| v.encrypted)
        };

        values
            .iter()
            .map(|(name, raw)| {
                let (source, secret) = chain
                    .iter()
                    .find_map(|f| {
                        defined(&f.variables, name).map(|s| (Source::Folder(f.name.clone()), s))
                    })
                    .or_else(|| {
                        personal.and_then(|e| {
                            defined(&e.variables, name).map(|s| (Source::Env(e.name.clone()), s))
                        })
                    })
                    .or_else(|| {
                        global
                            .and_then(|e| defined(&e.variables, name).map(|s| (Source::Global, s)))
                    })
                    .unwrap_or((Source::System, false));
                let value = resolve::resolve_str(raw, &values);
                (
                    name.clone(),
                    Var {
                        raw: raw.clone(),
                        value,
                        source,
                        secret,
                    },
                )
            })
            .collect()
    }
}

fn save(api: &ApiBackend, req: &HttpRequest) -> Result<(), YukiError> {
    api.requests.update_request(
        &req.workspace_id,
        &req.id,
        req.method.clone(),
        req.url.clone(),
        req.parameters.clone(),
        req.headers.clone(),
        req.body.clone(),
        req.auth.clone(),
    )
}

/// Copy Voleeo workspaces (+ their machine-local envs and key files) into the
/// Yuki data dir. Copies, never links: Yuki's format may diverge. Workspaces
/// already present are skipped. Returns how many were imported.
pub fn import_voleeo(dst: &Path) -> io::Result<usize> {
    let src = platform_data_dir().join("com.voleeo.desktop");
    let mut imported = 0;
    for entry in fs::read_dir(src.join("workspaces"))? {
        let entry = entry?;
        let id = entry.file_name();
        let target = dst.join("workspaces").join(&id);
        // `metadata` follows symlinks: synced workspaces are links to a git dir.
        if target.exists() || !fs::metadata(entry.path())?.is_dir() {
            continue;
        }
        copy_dir(&entry.path(), &target)?;
        let envs = src.join("envs-local").join(&id);
        if envs.is_dir() {
            copy_dir(&envs, &dst.join("envs-local").join(&id))?;
        }
        let key = format!("{}.key", id.to_string_lossy());
        if src.join("keys").join(&key).is_file() {
            fs::create_dir_all(dst.join("keys"))?;
            fs::copy(src.join("keys").join(&key), dst.join("keys").join(&key))?;
        }
        imported += 1;
    }
    Ok(imported)
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let path = entry.path();
        if fs::metadata(&path)?.is_dir() {
            copy_dir(&path, &to.join(&name))?;
        } else {
            fs::copy(&path, to.join(&name))?;
        }
    }
    Ok(())
}
