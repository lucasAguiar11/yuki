use super::request::{auth_arg, auth_to_store};
use super::{redact, ApiBackend};
use crate::protocol::ToolResult;
use serde_json::Value;

impl ApiBackend {
    pub(super) async fn folder_create(&self, args: &Value) -> ToolResult {
        let ws_id = require!(args, "workspaceId");
        let name = require!(args, "name");
        let folder_id = args["folderId"].as_str().map(str::to_string);
        let requests = self.requests.clone();
        let ws = ws_id.clone();
        match super::blocking(move || requests.create_folder(ws, folder_id, name)).await {
            Ok(f) => {
                self.notify_requests(&ws_id);
                ToolResult::json(&f)
            }
            Err(e) => ToolResult::error(e.to_string()),
        }
    }

    pub(super) async fn folder_rename(&self, args: &Value) -> ToolResult {
        let ws_id = require!(args, "workspaceId");
        let folder_id = require!(args, "folderId");
        let name = require!(args, "name");
        let requests = self.requests.clone();
        let ws = ws_id.clone();
        // Return the updated folder JSON (not bare text) for a uniform contract.
        let result = super::blocking(move || {
            requests.rename_folder(&ws, &folder_id, name)?;
            requests.get_folder(&ws, &folder_id)
        })
        .await;
        match result {
            Ok(folder) => {
                self.notify_requests(&ws_id);
                ToolResult::json(&folder)
            }
            Err(e) => ToolResult::error(e.to_string()),
        }
    }

    pub(super) async fn folder_set_auth(&self, args: &Value) -> ToolResult {
        let ws_id = require!(args, "workspaceId");
        let folder_id = require!(args, "folderId");
        let auth = match auth_arg(args) {
            Ok(Some(a)) => a,
            Ok(None) => return ToolResult::error("missing required field: auth"),
            Err(e) => return ToolResult::error(e),
        };
        let (requests, workspaces) = (self.requests.clone(), self.workspaces.clone());
        let app_data_dir = self.app_data_dir.clone();
        let ws = ws_id.clone();
        let result = super::blocking(move || {
            let mut folder = requests.get_folder(&ws, &folder_id)?;
            let auth = auth_to_store(&workspaces, &app_data_dir, &ws, auth, &mut folder.auth)?;
            requests.update_folder(&ws, &folder_id, folder.headers, auth)?;
            requests.get_folder(&ws, &folder_id)
        })
        .await;
        match result {
            Ok(mut folder) => {
                self.notify_requests(&ws_id);
                redact::mask_auth(&mut folder.auth);
                ToolResult::json(&folder)
            }
            Err(e) => ToolResult::error(e.to_string()),
        }
    }

    pub(super) async fn folder_delete(&self, args: &Value) -> ToolResult {
        let ws_id = require!(args, "workspaceId");
        let folder_id = require!(args, "folderId");
        let requests = self.requests.clone();
        let ws = ws_id.clone();
        let fid = folder_id.clone();
        // Cascade: removes the folder and every request/subfolder inside it
        // (mirrors the desktop's folder delete).
        match super::blocking(move || requests.delete_folder_cascade(&ws, &fid)).await {
            Ok(()) => {
                self.notify_requests(&ws_id);
                ToolResult::json(&serde_json::json!({ "deleted": folder_id }))
            }
            Err(e) => ToolResult::error(e.to_string()),
        }
    }
}
