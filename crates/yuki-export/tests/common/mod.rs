//! Shared fixture for the Yaak/Insomnia round trips and secret stripping.
#![allow(dead_code)]

use yuki_core::{
    ApiFolder, AuthConfig, BodyKind, Environment, EnvironmentKind, EnvironmentVariable,
    HttpRequest, InheritSource, RequestBody, RequestParameter, Workspace, WsConnection,
};
use yuki_export::Bundle;
use yuki_import::{ImportedFolder, ImportedItem, ImportedParam, ImportedRequest};

pub fn ts() -> String {
    "2026-01-01T00:00:00.000Z".to_string()
}

pub fn param(name: &str, value: &str) -> RequestParameter {
    RequestParameter {
        id: format!("p-{name}"),
        name: name.into(),
        value: value.into(),
        enabled: true,
    }
}

pub fn var(key: &str, value: &str, encrypted: bool) -> EnvironmentVariable {
    EnvironmentVariable {
        key: key.into(),
        value: value.into(),
        encrypted,
        enabled: true,
    }
}

pub fn env(
    id: &str,
    kind: EnvironmentKind,
    name: &str,
    vars: Vec<EnvironmentVariable>,
) -> Environment {
    Environment {
        id: id.into(),
        workspace_id: "ws00000001".into(),
        kind,
        name: name.into(),
        color: String::new(),
        shared: true,
        variables: vars,
        created_at: ts(),
        updated_at: ts(),
    }
}

pub fn http(name: &str, method: &str, url: &str, auth: AuthConfig, order: f64) -> HttpRequest {
    HttpRequest {
        id: format!("r{order}"),
        request_type: "api".into(),
        model: "http_request".into(),
        workspace_id: "ws00000001".into(),
        folder_id: Some("fusers".into()),
        method: method.into(),
        name: name.into(),
        url: url.into(),
        parameters: vec![],
        headers: vec![],
        body: None,
        auth,
        order,
        created_at: ts(),
        updated_at: ts(),
        variations: vec![],
        active_variation: None,
    }
}

/// Folder "Users" (bearer) holding "Create user" (json body, headers, path +
/// query params, basic auth) and "List users" (inherits); Global + "Dev" envs;
/// one WebSocket connection that HTTP-only formats must warn about.
pub fn bundle() -> Bundle {
    let mut create = http(
        "Create user",
        "POST",
        "{{ base_url }}/users/:id",
        AuthConfig::Basic {
            username: "ada".into(),
            password: "{{ pw }}".into(),
            password_encrypted: false,
            enabled: true,
        },
        0.0,
    );
    create.parameters = vec![param("id", "7"), param("verbose", "true")];
    create.headers = vec![
        param("Accept", "application/json"),
        param("X-Trace", "{{ trace }}"),
    ];
    create.body = Some(RequestBody {
        kind: BodyKind::Json,
        text: r#"{"name":"{{ name }}"}"#.into(),
        ..Default::default()
    });
    let list = http(
        "List users",
        "GET",
        "{{ base_url }}/users",
        AuthConfig::Inherit {
            from: InheritSource::Folder,
        },
        1.0,
    );

    Bundle {
        workspace: Workspace {
            id: "ws00000001".into(),
            name: "Demo API".into(),
            model: "workspace".into(),
            encrypted: false,
            sync_dir: None,
            key_check: None,
            headers: vec![param("X-Workspace", "demo")],
            auth: AuthConfig::None,
            dns_overrides: vec![],
            created_at: ts(),
            updated_at: ts(),
        },
        folders: vec![ApiFolder {
            id: "fusers".into(),
            folder_type: "api".into(),
            model: "folder".into(),
            workspace_id: "ws00000001".into(),
            folder_id: None,
            name: "Users".into(),
            headers: vec![],
            auth: AuthConfig::Bearer {
                token: "{{ token }}".into(),
                token_encrypted: false,
                enabled: true,
            },
            variables: vec![],
            color: None,
            order: 0.0,
            created_at: ts(),
            updated_at: ts(),
        }],
        requests: vec![list, create],
        ws: vec![WsConnection {
            id: "w1".into(),
            connection_type: "websocket".into(),
            model: "websocket".into(),
            workspace_id: "ws00000001".into(),
            folder_id: None,
            name: "Feed".into(),
            url: "wss://api.example.com/feed".into(),
            parameters: vec![],
            headers: vec![],
            auth: AuthConfig::None,
            order: 0.0,
            created_at: ts(),
            updated_at: ts(),
        }],
        grpc: vec![],
        environments: vec![
            env(
                "eglobal",
                EnvironmentKind::Global,
                "Global",
                vec![var("base_url", "https://api.example.com", false)],
            ),
            env(
                "edev",
                EnvironmentKind::Personal,
                "Dev",
                vec![var("token", "dev-token", true)],
            ),
        ],
    }
}

pub fn folder(items: &[ImportedItem], name: &str) -> ImportedFolder {
    items
        .iter()
        .find_map(|i| match i {
            ImportedItem::Folder(f) if f.name == name => Some(f.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("folder {name} missing"))
}

pub fn request(items: &[ImportedItem], name: &str) -> ImportedRequest {
    items
        .iter()
        .find_map(|i| match i {
            ImportedItem::Request(r) if r.name == name => Some(r.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("request {name} missing"))
}

pub fn pairs(params: &[ImportedParam]) -> Vec<(&str, &str)> {
    params
        .iter()
        .map(|p| (p.name.as_str(), p.value.as_str()))
        .collect()
}
