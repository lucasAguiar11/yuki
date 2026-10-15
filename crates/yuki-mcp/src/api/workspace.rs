use super::request::{auth_arg, auth_to_store};
use super::{redact, ApiBackend};
use crate::protocol::ToolResult;
use serde_json::Value;

impl ApiBackend {
    pub(super) async fn workspace_list(&self, args: &Value) -> ToolResult {
        let reveal = redact::reveal(args);
        let workspaces = self.workspaces.clone();
        super::run_blocking(move || match workspaces.list() {
            Ok(mut ws) => {
                if !reveal {
                    for w in ws.iter_mut() {
                        redact::mask_auth(&mut w.auth);
                    }
                }
                ToolResult::json(&ws)
            }
            Err(e) => ToolResult::error(e.to_string()),
        })
        .await
    }

    pub(super) async fn workspace_create(&self, args: &Value) -> ToolResult {
        let name = require!(args, "name");
        let encrypted = args["encrypted"].as_bool().unwrap_or(false);
        let workspaces = self.workspaces.clone();
        match super::blocking(move || workspaces.create(name, encrypted)).await {
            Ok(ws) => ToolResult::json(&ws),
            Err(e) => ToolResult::error(e.to_string()),
        }
    }

    pub(super) async fn workspace_set_auth(&self, args: &Value) -> ToolResult {
        let ws_id = require!(args, "workspaceId");
        let auth = match auth_arg(args) {
            Ok(Some(a)) => a,
            Ok(None) => return ToolResult::error("missing required field: auth"),
            Err(e) => return ToolResult::error(e),
        };
        let workspaces = self.workspaces.clone();
        let app_data_dir = self.app_data_dir.clone();
        let result = super::blocking(move || {
            let mut stored = workspaces.get(&ws_id)?.auth;
            let auth = auth_to_store(&workspaces, &app_data_dir, &ws_id, auth, &mut stored)?;
            workspaces.update_auth(&ws_id, auth)?;
            workspaces.get(&ws_id)
        })
        .await;
        match result {
            Ok(mut ws) => {
                redact::mask_auth(&mut ws.auth);
                ToolResult::json(&ws)
            }
            Err(e) => ToolResult::error(e.to_string()),
        }
    }
}
