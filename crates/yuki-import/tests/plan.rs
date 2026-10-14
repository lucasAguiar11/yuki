//! `plan()` end to end: fresh ids, selection, auth/header flattening, named
//! environments, and the Voleeo bundle path.

use yuki_core::{AuthConfig, HttpRequest};
use yuki_import::{detect_format, plan, preview, ConvertedPlan, ImportFormat};

const V5: &str = include_str!("fixtures/insomnia/export_v5.yaml");
const YAAK: &str = include_str!("fixtures/yaak/export.json");
const BUNDLE: &str = include_str!("fixtures/voleeo/bundle.yaml");

fn req<'a>(p: &'a ConvertedPlan, name: &str) -> &'a HttpRequest {
    p.requests.iter().find(|r| r.name == name).unwrap()
}

fn header<'a>(r: &'a HttpRequest, name: &str) -> Vec<&'a str> {
    r.headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
        .collect()
}

fn ids(sel: &[&str]) -> Vec<String> {
    sel.iter().map(|s| s.to_string()).collect()
}

#[test]
fn fresh_ids_remapped_parents_and_flat_folders() {
    let p = plan(ImportFormat::Insomnia, V5, None, "ws_target", 10.0).unwrap();
    assert_eq!(p.folders.len(), 1);
    assert_eq!(p.requests.len(), 3);
    let users = &p.folders[0];
    assert_eq!(users.workspace_id, "ws_target");
    assert_eq!(users.auth, AuthConfig::None);
    assert!(users.headers.is_empty());
    assert_eq!(
        req(&p, "List users").folder_id.as_deref(),
        Some(users.id.as_str())
    );
    assert_eq!(req(&p, "Health").folder_id, None);
    assert!(p
        .requests
        .iter()
        .all(|r| r.id.len() == 8 && r.order >= 10.0));
}

#[test]
fn selection_is_honored() {
    // Node "0.1" = Users › Create user.
    let p = plan(ImportFormat::Insomnia, V5, Some(&ids(&["0.1"])), "ws", 0.0).unwrap();
    assert_eq!(p.folders.len(), 1);
    assert_eq!(p.requests.len(), 1);
    assert_eq!(p.requests[0].name, "Create user");
}

#[test]
fn folder_auth_and_headers_flatten_onto_requests() {
    let p = plan(ImportFormat::Insomnia, V5, None, "ws", 0.0).unwrap();

    // Folder bearer reaches the inheriting request.
    let list = req(&p, "List users");
    assert!(matches!(&list.auth, AuthConfig::Bearer { token, .. } if token == "{{ token }}"));
    // Request header `accept` replaces the folder's `Accept` (case-insensitive);
    // folder headers come first.
    assert_eq!(header(list, "accept"), vec!["application/xml"]);
    assert_eq!(list.headers[0].name, "X-Tenant");
    assert_eq!(list.headers.last().unwrap().name, "accept");

    // The request's own auth wins over the folder's.
    assert!(matches!(
        req(&p, "Create user").auth,
        AuthConfig::Basic { .. }
    ));
    // Outside the folder: no auth, no folder headers.
    let health = req(&p, "Health");
    assert_eq!(health.auth, AuthConfig::None);
    assert!(health.headers.is_empty());
}

#[test]
fn sub_environments_become_environments() {
    let p = plan(ImportFormat::Insomnia, V5, None, "ws_target", 0.0).unwrap();
    assert_eq!(p.environments.len(), 1);
    let staging = &p.environments[0];
    assert_eq!(staging.name, "Staging");
    assert_eq!(staging.workspace_id, "ws_target");
    assert!(!staging.shared);
    assert_eq!(staging.variables[0].key, "base_url");
    assert!(p.variables.iter().any(|v| v.key == "token"));

    let y = plan(ImportFormat::Yaak, YAAK, None, "ws_target", 0.0).unwrap();
    assert!(y.environments.iter().any(|e| e.name == "Sub"));
}

#[test]
fn voleeo_bundle_plan() {
    assert_eq!(detect_format(BUNDLE), Some(ImportFormat::Voleeo));
    let p = plan(ImportFormat::Voleeo, BUNDLE, None, "ws_target", 0.0).unwrap();

    // Two workspaces → one top-level folder each, wrapping their trees.
    let alpha = p.folders.iter().find(|f| f.name == "Alpha").unwrap();
    let beta = p.folders.iter().find(|f| f.name == "Beta").unwrap();
    let users = p.folders.iter().find(|f| f.name == "Users").unwrap();
    assert_eq!(p.folders.len(), 4);
    assert!(alpha.folder_id.is_none() && beta.folder_id.is_none());
    assert_eq!(users.folder_id.as_deref(), Some(alpha.id.as_str()));
    assert_eq!(users.color.as_deref(), Some("#ff8800"));
    assert!(p.folders.iter().all(|f| f.workspace_id == "ws_target"
        && f.auth == AuthConfig::None
        && f.headers.is_empty()));
    assert!(p.folders.iter().all(|f| f.id != "fld_users"));

    // Workspace bearer + headers flatten; nearer `x-client` replaces `X-Client`.
    let list = req(&p, "List users");
    assert_ne!(list.id, "req_list");
    assert_eq!(list.workspace_id, "ws_target");
    assert_eq!(list.folder_id.as_deref(), Some(users.id.as_str()));
    assert!(matches!(&list.auth, AuthConfig::Bearer { token, .. } if token == "ws-token"));
    assert_eq!(header(list, "x-client"), vec!["users"]);
    let ping = req(&p, "Ping");
    assert_eq!(ping.folder_id.as_deref(), Some(alpha.id.as_str()));
    assert!(matches!(&ping.auth, AuthConfig::Bearer { token, .. } if token == "ws-token"));
    assert_eq!(header(ping, "x-client"), vec!["yuki"]);
    assert!(
        matches!(&req(&p, "List orders").auth, AuthConfig::Basic { username, .. } if username == "bob")
    );

    // WS + gRPC kept, remapped.
    assert_eq!(p.websockets.len(), 1);
    let ws = &p.websockets[0];
    assert_ne!(ws.id, "ws_live");
    assert_eq!(ws.workspace_id, "ws_target");
    assert_eq!(ws.folder_id.as_deref(), Some(users.id.as_str()));
    assert!(matches!(ws.auth, AuthConfig::Bearer { .. }));
    assert_eq!(p.grpc.len(), 1);
    assert_eq!(p.grpc[0].folder_id.as_deref(), Some(alpha.id.as_str()));

    // Global env → variables; personal env kept with its flags.
    assert!(p.variables.iter().any(|v| v.key == "base_url"));
    assert_eq!(p.environments.len(), 1);
    let dev = &p.environments[0];
    assert_ne!(dev.id, "env_dev");
    assert_eq!(dev.workspace_id, "ws_target");
    assert!(dev.shared);
    assert_eq!(dev.color, "#00aa00");
    assert!(dev.variables[0].encrypted);

    assert!(p.warnings.iter().any(|w| w.contains("DNS")));
}

#[test]
fn voleeo_bundle_selection() {
    // Only Beta › Orders › List orders: Alpha (and its ws/gRPC) drop out.
    let p = plan(
        ImportFormat::Voleeo,
        BUNDLE,
        Some(&ids(&["1.0.0"])),
        "ws",
        0.0,
    )
    .unwrap();
    let names: Vec<&str> = p.folders.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Beta", "Orders"]);
    assert_eq!(p.requests.len(), 1);
    assert!(p.websockets.is_empty() && p.grpc.is_empty());

    // Selecting Users keeps its WS connection; the root gRPC follows Alpha.
    let p = plan(
        ImportFormat::Voleeo,
        BUNDLE,
        Some(&ids(&["0.0.0"])),
        "ws",
        0.0,
    )
    .unwrap();
    assert_eq!(p.websockets.len(), 1);
    assert_eq!(p.grpc.len(), 1);
}

#[test]
fn voleeo_bundle_auth_flags_cleared_and_unknown_version_warned() {
    let edited = BUNDLE
        .replace("voleeoBundle: \"1.0\"", "voleeoBundle: \"9.9\"")
        .replace(
            "{ kind: bearer, token: ws-token }",
            "{ kind: bearer, token: ws-token, token_encrypted: true }",
        );
    let p = plan(ImportFormat::Voleeo, &edited, None, "ws", 0.0).unwrap();
    let bearer = p
        .requests
        .iter()
        .find_map(|r| match &r.auth {
            AuthConfig::Bearer {
                token,
                token_encrypted,
                ..
            } if token == "ws-token" => Some(*token_encrypted),
            _ => None,
        })
        .unwrap();
    assert!(!bearer);
    assert!(p.warnings.iter().any(|w| w.contains("9.9")));
}

#[test]
fn voleeo_preview() {
    let pv = preview(ImportFormat::Voleeo, BUNDLE).unwrap();
    assert_eq!(pv.format, ImportFormat::Voleeo);
    assert_eq!(pv.format_version.as_deref(), Some("1.0"));
    assert_eq!(pv.tree.len(), 2);
    let alpha = &pv.tree[0];
    assert_eq!((alpha.id.as_str(), alpha.name.as_str()), ("0", "Alpha"));
    assert_eq!(alpha.children[0].id, "0.0");
    assert_eq!(alpha.children[0].children[0].id, "0.0.0");
    assert_eq!(alpha.children[0].children[0].kind, "request");
    assert_eq!(alpha.children[1].name, "Ping");
    assert_eq!(pv.tree[1].children[0].children[0].name, "List orders");
    assert_eq!(pv.variable_count, 2);
    assert_eq!(pv.warnings.len(), 1);
}
