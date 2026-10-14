//! Insomnia v5 (`collection.insomnia.rest/5.0`, YAML) → IR; design documents
//! (`spec.…`) and environment files (`environment.…`, no `collection`) share
//! the same shape. Unlike v4 the tree is already nested: `collection[]` holds requests and folders (`children[]`) in
//! display order. Request/auth/body/header shapes match v4, so the v4 helpers
//! map them; `parse_insomnia` has already normalized `{{ _.x }}` → `{{ x }}`.

use crate::insomnia::{build_request, headers_of, to_auth, Notes};
use crate::ir::*;
use crate::util::name_of;
use serde_json::Value;

pub const V5_TYPE: &str = "collection.insomnia.rest/5.0";

pub fn parse(doc: &Value, mut warnings: Vec<String>) -> ImportedCollection {
    let envs = doc.get("environments");
    let mut notes = Notes::default();
    let items = build_items(doc.get("collection"), &mut notes);
    let variables = envs.map(|e| notes.env_of(e)).unwrap_or_default();
    let environments = envs
        .and_then(|e| e.get("subEnvironments"))
        .and_then(Value::as_array)
        .map(|subs| {
            subs.iter()
                .map(|e| ImportedEnvironment {
                    name: name_of(e),
                    variables: notes.env_of(e),
                })
                .collect()
        })
        .unwrap_or_default();
    notes.warn(&mut warnings);
    ImportedCollection {
        name: doc
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Imported API")
            .to_string(),
        version: Some("5".into()),
        variables,
        environments,
        root_auth: ImportedAuth::None,
        root_headers: Vec::new(),
        items,
        warnings,
    }
}

/// WebSocket (`ws-req_*`) and gRPC (`greq_*`) items have neither `method` nor
/// `children`; they're counted in `notes.skipped` instead of becoming GET requests.
fn build_items(items: Option<&Value>, notes: &mut Notes) -> Vec<ImportedItem> {
    let Some(items) = items.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for it in items {
        notes.script(it);
        if it.get("children").is_none() || it.get("method").is_some() {
            let id = it.pointer("/meta/id").and_then(Value::as_str).unwrap_or("");
            if it.get("method").is_some() || id.starts_with("req_") {
                out.push(ImportedItem::Request(build_request(it)));
            } else {
                notes.skipped += 1;
            }
            continue;
        }
        out.push(ImportedItem::Folder(ImportedFolder {
            name: name_of(it),
            description: it
                .pointer("/meta/description")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            auth: it.get("authentication").map(to_auth).unwrap_or_default(),
            headers: headers_of(it),
            variables: notes.env(it.get("environment")),
            items: build_items(it.get("children"), notes),
        }));
    }
    out
}
