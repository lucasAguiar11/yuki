//! Request CRUD: the `req_*.yaml` half of `RequestStore`, plus each request's
//! markdown notes in a sidecar `req_*.md`.

use super::{save_request_if_changed, RequestStore};
use chrono::Utc;
use std::path::PathBuf;
use yuki_core::{new_id, AuthConfig, HttpRequest, RequestBody, RequestParameter, YukiError};

impl RequestStore {
    fn req_path(&self, workspace_id: &str, id: &str) -> Result<PathBuf, YukiError> {
        crate::validate_id(workspace_id)?;
        crate::validate_id(id)?;
        Ok(self
            .workspaces_dir
            .join(workspace_id)
            .join(format!("req_{id}.yaml")))
    }

    /// Notes sit beside the YAML so editors and GitHub render them as markdown.
    fn notes_path(&self, workspace_id: &str, id: &str) -> Result<PathBuf, YukiError> {
        Ok(self.req_path(workspace_id, id)?.with_extension("md"))
    }

    /// The request's markdown notes; `""` when it has none.
    pub fn get_notes(&self, workspace_id: &str, id: &str) -> Result<String, YukiError> {
        match std::fs::read_to_string(self.notes_path(workspace_id, id)?) {
            Ok(s) => Ok(s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(YukiError::Storage(e.to_string())),
        }
    }

    /// Writes only on change; blank notes remove the file (no empty files in git).
    pub fn save_notes(&self, workspace_id: &str, id: &str, notes: &str) -> Result<(), YukiError> {
        let path = self.notes_path(workspace_id, id)?;
        if notes.trim().is_empty() {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    Err(YukiError::Storage(e.to_string()))
                }
                _ => Ok(()),
            };
        }
        if self.get_notes(workspace_id, id)? == notes {
            return Ok(());
        }
        crate::write_atomic(path, notes)
    }

    pub fn get_request(&self, workspace_id: &str, id: &str) -> Result<HttpRequest, YukiError> {
        let path = self.req_path(workspace_id, id)?;
        if !path.exists() {
            return Err(YukiError::NotFound(format!("request {id}")));
        }
        let content =
            std::fs::read_to_string(&path).map_err(|e| YukiError::Storage(e.to_string()))?;
        let req: HttpRequest =
            serde_yaml::from_str(&content).map_err(|e| YukiError::Storage(e.to_string()))?;
        if req.id != id {
            return Err(YukiError::Storage(format!(
                "request file id mismatch for {id}"
            )));
        }
        Ok(req)
    }

    pub fn list_requests(&self, workspace_id: &str) -> Result<Vec<HttpRequest>, YukiError> {
        let dir = self.workspace_dir(workspace_id)?;
        let mut items = Vec::new();
        for entry in std::fs::read_dir(&dir)
            .map_err(|e| YukiError::Storage(e.to_string()))?
            .flatten()
        {
            let name = entry.file_name();
            let filename = name.to_string_lossy();
            if !filename.starts_with("req_") || !filename.ends_with(".yaml") {
                continue;
            }
            let content = std::fs::read_to_string(entry.path())
                .map_err(|e| YukiError::Storage(e.to_string()))?;
            let req: HttpRequest =
                serde_yaml::from_str(&content).map_err(|e| YukiError::Storage(e.to_string()))?;
            items.push(req);
        }
        // Order ascending; created_at breaks ties.
        items.sort_by(|a, b| {
            a.order
                .partial_cmp(&b.order)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.created_at.cmp(&b.created_at))
        });
        Ok(items)
    }

    pub fn create_request(
        &self,
        workspace_id: String,
        folder_id: Option<String>,
        name: String,
        method: String,
        url: String,
    ) -> Result<HttpRequest, YukiError> {
        self.workspace_dir(&workspace_id)?;
        let id = new_id();
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%S%.6f").to_string();
        let order = Utc::now().timestamp_millis() as f64;
        let req = HttpRequest {
            id: id.clone(),
            request_type: "api".to_string(),
            model: "http_request".to_string(),
            workspace_id: workspace_id.clone(),
            folder_id,
            method,
            name,
            url,
            parameters: vec![],
            headers: vec![],
            body: None,
            auth: AuthConfig::None,
            order,
            created_at: now.clone(),
            updated_at: now,
        };
        let content = serde_yaml::to_string(&req).map_err(|e| YukiError::Storage(e.to_string()))?;
        crate::write_atomic(self.req_path(&workspace_id, &id)?, content)?;
        Ok(req)
    }

    /// Clone `src` into a fresh request file (caller picks folder, name, order).
    /// `pub(super)` so the folder-copy recursion can reach it from its sibling.
    pub(super) fn copy_request(
        &self,
        src: &HttpRequest,
        folder_id: Option<String>,
        name: String,
        order: f64,
    ) -> Result<HttpRequest, YukiError> {
        let id = new_id();
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%S%.6f").to_string();
        let req = HttpRequest {
            id: id.clone(),
            request_type: src.request_type.clone(),
            model: src.model.clone(),
            workspace_id: src.workspace_id.clone(),
            folder_id,
            method: src.method.clone(),
            name,
            url: src.url.clone(),
            parameters: src.parameters.clone(),
            headers: src.headers.clone(),
            body: src.body.clone(),
            auth: src.auth.clone(),
            order,
            created_at: now.clone(),
            updated_at: now,
        };
        let content = serde_yaml::to_string(&req).map_err(|e| YukiError::Storage(e.to_string()))?;
        crate::write_atomic(self.req_path(&req.workspace_id, &id)?, content)?;
        let notes = self.get_notes(&src.workspace_id, &src.id)?;
        self.save_notes(&req.workspace_id, &id, &notes)?;
        Ok(req)
    }

    /// Duplicate a request as a sibling directly below the original,
    /// named "Copy of {name}".
    pub fn duplicate_request(
        &self,
        workspace_id: &str,
        id: &str,
    ) -> Result<HttpRequest, YukiError> {
        let src = self.get_request(workspace_id, id)?;
        let order = self.order_after(workspace_id, src.folder_id.as_deref(), src.order)?;
        self.copy_request(
            &src,
            src.folder_id.clone(),
            format!("Copy of {}", src.name),
            order,
        )
    }

    pub fn rename_request(
        &self,
        workspace_id: &str,
        id: &str,
        name: String,
    ) -> Result<(), YukiError> {
        let path = self.req_path(workspace_id, id)?;
        let content =
            std::fs::read_to_string(&path).map_err(|e| YukiError::Storage(e.to_string()))?;
        let req: HttpRequest =
            serde_yaml::from_str(&content).map_err(|e| YukiError::Storage(e.to_string()))?;
        let mut next = req.clone();
        next.name = name;
        save_request_if_changed(&path, &req, next)
    }

    // ponytail: 8 fields are the request itself; a struct just adds an indirection
    #[allow(clippy::too_many_arguments)]
    pub fn update_request(
        &self,
        workspace_id: &str,
        id: &str,
        method: String,
        url: String,
        parameters: Vec<RequestParameter>,
        headers: Vec<RequestParameter>,
        body: Option<RequestBody>,
        auth: AuthConfig,
    ) -> Result<(), YukiError> {
        let path = self.req_path(workspace_id, id)?;
        let content =
            std::fs::read_to_string(&path).map_err(|e| YukiError::Storage(e.to_string()))?;
        let req: HttpRequest =
            serde_yaml::from_str(&content).map_err(|e| YukiError::Storage(e.to_string()))?;
        let mut next = req.clone();
        next.method = method;
        next.url = url;
        next.parameters = parameters;
        next.headers = headers;
        next.body = body;
        next.auth = auth;
        save_request_if_changed(&path, &req, next)
    }

    pub fn delete_request(&self, workspace_id: &str, id: &str) -> Result<(), YukiError> {
        self.save_notes(workspace_id, id, "")?;
        let path = self.req_path(workspace_id, id)?;
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| YukiError::Storage(e.to_string()))?;
        }
        Ok(())
    }

    pub fn update_request_position(
        &self,
        workspace_id: &str,
        id: &str,
        folder_id: Option<String>,
        order: f64,
    ) -> Result<(), YukiError> {
        let path = self.req_path(workspace_id, id)?;
        let content =
            std::fs::read_to_string(&path).map_err(|e| YukiError::Storage(e.to_string()))?;
        let req: HttpRequest =
            serde_yaml::from_str(&content).map_err(|e| YukiError::Storage(e.to_string()))?;
        let mut next = req.clone();
        next.folder_id = folder_id;
        next.order = order;
        save_request_if_changed(&path, &req, next)
    }
}
