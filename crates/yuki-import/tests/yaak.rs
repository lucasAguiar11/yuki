//! Fixture-driven tests for the Yaak export (JSON) parser → IR mapping.

use serde_json::json;
use yuki_core::AuthConfig;
use yuki_import::{
    detect_format, parse, ImportFormat, ImportedAuth, ImportedBody, ImportedItem, RawKind,
};

const EXPORT: &str = include_str!("fixtures/yaak/export.json");

fn sample() -> yuki_import::ImportedCollection {
    parse(ImportFormat::Yaak, EXPORT).unwrap()
}

#[test]
fn detects_yaak() {
    assert_eq!(detect_format(EXPORT), Some(ImportFormat::Yaak));
}

#[test]
fn maps_name_version_base_env_and_root_auth() {
    let col = sample();
    assert_eq!(col.name, "Sample API");
    assert_eq!(col.version.as_deref(), Some("4"));

    let base = col.variables.iter().find(|v| v.key == "base_url").unwrap();
    assert_eq!(base.value, "https://api.example.com");
    // Workspace bearer → root auth; `${[ token ]}` → `{{ token }}`.
    let ImportedAuth::Bearer { token } = &col.root_auth else {
        panic!("expected bearer root auth, got {:?}", col.root_auth);
    };
    assert_eq!(token, "{{ token }}");

    // The sub-environment is imported as its own named environment.
    let sub = col.environments.iter().find(|e| e.name == "Sub").unwrap();
    assert!(sub.variables.iter().any(|v| v.key == "x"));
}

#[test]
fn reconstructs_tree_by_folder_id_ordered_by_sort_priority() {
    let col = sample();
    let names: Vec<String> = col
        .items
        .iter()
        .map(|i| match i {
            ImportedItem::Folder(f) => f.name.clone(),
            ImportedItem::Request(r) => r.name.clone(),
        })
        .collect();
    assert_eq!(names, vec!["Users", "Upload avatar", "GraphQL"]);

    let users = col
        .items
        .iter()
        .find_map(|i| match i {
            ImportedItem::Folder(f) if f.name == "Users" => Some(f),
            _ => None,
        })
        .unwrap();
    assert_eq!(users.items.len(), 2);
    assert!(matches!(users.auth, ImportedAuth::Inherit));
}

#[test]
fn converts_templates_and_splits_path_query() {
    let col = sample();
    let get = all_requests(&col)
        .into_iter()
        .find(|r| r.name == "Get user")
        .unwrap();
    assert_eq!(get.url, "{{ base_url }}/users/:id");
    assert!(get.path_params.iter().any(|p| p.name == "id"));
    assert!(get.query.iter().any(|p| p.name == "expand" && p.enabled));
    assert!(get.query.iter().any(|p| p.name == "fields" && !p.enabled));
    assert!(matches!(get.auth, ImportedAuth::Inherit));
}

#[test]
fn maps_bodies_and_explicit_auth() {
    let col = sample();
    let reqs = all_requests(&col);

    let create = reqs.iter().find(|r| r.name == "Create user").unwrap();
    let Some(ImportedBody::Raw {
        hint: RawKind::Json,
        text,
    }) = &create.body
    else {
        panic!("expected json body, got {:?}", create.body);
    };
    assert!(text.contains("Ada"));

    let upload = reqs.iter().find(|r| r.name == "Upload avatar").unwrap();
    let Some(ImportedBody::Multipart(fields)) = &upload.body else {
        panic!("expected multipart, got {:?}", upload.body);
    };
    assert!(fields.iter().any(|f| f.name == "file" && f.is_file));
    assert!(matches!(upload.auth, ImportedAuth::Basic { .. }));

    let gql = reqs.iter().find(|r| r.name == "GraphQL").unwrap();
    let Some(ImportedBody::GraphQl { query, .. }) = &gql.body else {
        panic!("expected graphql, got {:?}", gql.body);
    };
    assert!(query.contains("me"));
    // Explicit `"none"` → None (not inherit).
    assert!(matches!(gql.auth, ImportedAuth::None));
}

fn plan_of(doc: serde_json::Value) -> yuki_import::ConvertedPlan {
    yuki_import::plan(ImportFormat::Yaak, &doc.to_string(), None, "ws", 0.0).unwrap()
}

fn auth_of<'a>(p: &'a yuki_import::ConvertedPlan, name: &str) -> &'a AuthConfig {
    &p.requests.iter().find(|r| r.name == name).unwrap().auth
}

#[test]
fn missing_auth_inherits_but_none_stops_and_workspace_headers_apply() {
    let p = plan_of(json!({ "yaakSchema": 4, "resources": {
        "workspaces": [{ "id": "w", "name": "W", "authenticationType": "bearer",
            "authentication": { "token": "t" }, "headers": [{ "name": "X-WS", "value": "1" }] }],
        "folders": [{ "id": "f", "name": "F", "authenticationType": "none" }],
        "httpRequests": [
            { "id": "a", "name": "root", "url": "http://x" },
            { "id": "b", "name": "in none folder", "folderId": "f", "url": "http://x" },
            { "id": "c", "name": "disabled", "url": "http://x", "authenticationType": "basic",
              "authentication": { "disabled": true } }
        ] } }));
    assert!(matches!(auth_of(&p, "root"), AuthConfig::Bearer { token, .. } if token == "t"));
    assert_eq!(auth_of(&p, "in none folder"), &AuthConfig::None);
    assert_eq!(auth_of(&p, "disabled"), &AuthConfig::None);
    assert!(p
        .requests
        .iter()
        .all(|r| r.headers.iter().any(|h| h.name == "X-WS")));
}

#[test]
fn legacy_schemas_keep_requests_and_global_variables() {
    // Schema 1: `requests`, base variables on the workspace.
    let v1 = plan_of(json!({ "yaakSchema": 1, "resources": {
        "workspaces": [{ "id": "w", "name": "W", "variables": [{ "name": "host", "value": "h" }] }],
        "requests": [{ "id": "a", "name": "r", "url": "http://x" }] } }));
    assert_eq!(v1.requests.len(), 1);
    assert_eq!(v1.variables[0].key, "host");
    // Schema 3 (`environmentId: null`) and early 4 (`base: true`) mark the base env.
    for base in [json!({ "environmentId": null }), json!({ "base": true })] {
        let mut env = json!({ "id": "e", "name": "Global Variables",
            "variables": [{ "name": "k", "value": "v" }, { "name": "off", "value": "o", "enabled": false }] });
        env.as_object_mut()
            .unwrap()
            .extend(base.as_object().unwrap().clone());
        let p = plan_of(json!({ "yaakSchema": 4, "resources": { "environments": [env] } }));
        assert!(p.environments.is_empty());
        assert_eq!(p.variables.len(), 2);
        assert!(!p.variables.iter().find(|v| v.key == "off").unwrap().enabled);
    }
}

#[test]
fn maps_plugin_auths_file_paths_and_folder_envs() {
    let p = plan_of(json!({ "yaakSchema": 4, "resources": {
        "folders": [{ "id": "f", "name": "F" }],
        "environments": [{ "id": "e", "name": "F env", "parentModel": "folder", "parentId": "f",
            "variables": [{ "name": "fv", "value": "1" }] }],
        "httpRequests": [
            { "id": "a", "name": "aws", "url": "http://x", "authenticationType": "awsv4",
              "authentication": { "accessKeyId": "AK", "secretAccessKey": "SK", "region": "r", "service": "s" } },
            { "id": "b", "name": "ntlm", "url": "http://x", "authenticationType": "windows",
              "authentication": { "username": "u", "password": "p", "domain": "D", "workstation": "W" } },
            { "id": "c", "name": "prefix", "url": "http://x", "authenticationType": "bearer",
              "authentication": { "token": "t", "prefix": "Token" } },
            { "id": "d", "name": "jwt", "url": "http://x", "authenticationType": "jwt" },
            { "id": "g", "name": "upload", "url": "http://x", "bodyType": "multipart/form-data",
              "body": { "form": [{ "name": "f", "file": "/tmp/a.png", "contentType": "image/png" }] } },
            { "id": "h", "name": "bin", "url": "http://x", "bodyType": "binary",
              "body": { "filePath": "/tmp/a.bin" } }
        ] } }));
    assert!(
        matches!(auth_of(&p, "aws"), AuthConfig::AwsSigV4 { access_key, region, .. } if access_key == "AK" && region == "r")
    );
    assert!(
        matches!(auth_of(&p, "ntlm"), AuthConfig::Ntlm { domain, workstation, .. } if domain == "D" && workstation == "W")
    );
    assert!(
        matches!(auth_of(&p, "prefix"), AuthConfig::ApiKey { key, value, .. } if key == "Authorization" && value == "Token t")
    );
    assert_eq!(auth_of(&p, "jwt"), &AuthConfig::None);
    assert!(p.warnings.iter().any(|w| w.contains("`jwt`")));

    let field = &p
        .requests
        .iter()
        .find(|r| r.name == "upload")
        .unwrap()
        .body
        .as_ref()
        .unwrap()
        .fields
        .as_ref()
        .unwrap()[0];
    assert!(
        field.is_file
            && field.value == "/tmp/a.png"
            && field.content_type.as_deref() == Some("image/png")
    );
    let bin = p
        .requests
        .iter()
        .find(|r| r.name == "bin")
        .unwrap()
        .body
        .as_ref()
        .unwrap();
    assert_eq!(bin.file_path.as_deref(), Some("/tmp/a.bin"));
    assert_eq!(p.folders[0].variables[0].key, "fv");
    assert!(p.environments.is_empty());
}

fn all_requests(col: &yuki_import::ImportedCollection) -> Vec<yuki_import::ImportedRequest> {
    fn walk(items: &[ImportedItem], out: &mut Vec<yuki_import::ImportedRequest>) {
        for it in items {
            match it {
                ImportedItem::Request(r) => out.push(r.clone()),
                ImportedItem::Folder(f) => walk(&f.items, out),
            }
        }
    }
    let mut out = Vec::new();
    walk(&col.items, &mut out);
    out
}
