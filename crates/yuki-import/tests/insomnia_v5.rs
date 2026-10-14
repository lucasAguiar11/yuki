//! Insomnia v5 (YAML, nested `collection`) → IR.

use yuki_import::{
    detect_format, parse, ImportFormat, ImportedAuth, ImportedBody, ImportedCollection,
    ImportedItem, RawKind,
};

const V5: &str = include_str!("fixtures/insomnia/export_v5.yaml");

fn sample() -> ImportedCollection {
    parse(ImportFormat::Insomnia, V5).unwrap()
}

#[test]
fn detects_v5_as_insomnia() {
    assert_eq!(detect_format(V5), Some(ImportFormat::Insomnia));
}

#[test]
fn maps_name_envs_and_tree() {
    let col = sample();
    assert_eq!(col.name, "Sample API");
    assert_eq!(col.version.as_deref(), Some("5"));
    assert!(col
        .variables
        .iter()
        .any(|v| v.key == "base_url" && v.value == "https://api.example.com"));
    assert_eq!(col.environments.len(), 1);
    assert_eq!(col.environments[0].name, "Staging");
    assert_eq!(col.environments[0].variables[0].key, "base_url");

    assert_eq!(col.items.len(), 2);
    let ImportedItem::Folder(users) = &col.items[0] else {
        panic!("expected Users folder first");
    };
    assert_eq!(users.name, "Users");
    assert_eq!(users.description.as_deref(), Some("User management"));
    assert!(matches!(&users.auth, ImportedAuth::Bearer { token } if token == "{{ token }}"));
    assert_eq!(users.headers.len(), 2);
    assert_eq!(users.items.len(), 2);
    assert!(matches!(col.items[1], ImportedItem::Request(_)));
}

#[test]
fn maps_requests() {
    let col = sample();
    let ImportedItem::Folder(users) = &col.items[0] else {
        panic!("expected folder");
    };
    let ImportedItem::Request(list) = &users.items[0] else {
        panic!("expected request");
    };
    assert_eq!(list.method, "GET");
    assert_eq!(list.url, "{{ base_url }}/users");
    assert!(list.query.iter().any(|p| p.name == "debug" && !p.enabled));
    assert!(matches!(list.auth, ImportedAuth::Inherit));

    let ImportedItem::Request(create) = &users.items[1] else {
        panic!("expected request");
    };
    assert_eq!(create.method, "POST");
    let Some(ImportedBody::Raw {
        hint: RawKind::Json,
        text,
    }) = &create.body
    else {
        panic!("expected json body, got {:?}", create.body);
    };
    assert!(text.contains("{{ user_name }}"));
    assert!(matches!(create.auth, ImportedAuth::Basic { .. }));
}

#[test]
fn skips_websocket_and_grpc_items_with_warning() {
    let col = sample();
    // Users folder + Health only; `ws-req_*` / `greq_*` don't become requests.
    assert_eq!(col.items.len(), 2);
    assert!(col
        .warnings
        .iter()
        .any(|w| w == "2 WebSocket/gRPC item(s) skipped"));
}

#[test]
fn imports_design_documents_and_environment_files() {
    let spec = r#"
type: spec.insomnia.rest/5.0
name: Baggage
collection:
  - name: bags
    children:
      - name: List
        method: GET
        url: "{{ _.base_url }}/bags/:id"
        pathParameters:
          - name: id
            value: "7"
        scripts:
          preRequest: "insomnia.environment.set('a', 1)"
environments:
  name: Base
  data:
    base_url: http://localhost
    obj:
      inner: x
spec:
  contents: {}
"#;
    assert_eq!(detect_format(spec), Some(ImportFormat::Insomnia));
    let col = parse(ImportFormat::Insomnia, spec).unwrap();
    let ImportedItem::Folder(bags) = &col.items[0] else {
        panic!("expected folder");
    };
    let ImportedItem::Request(list) = &bags.items[0] else {
        panic!("expected request");
    };
    assert_eq!(list.url, "{{ base_url }}/bags/:id");
    assert!(list
        .path_params
        .iter()
        .any(|p| p.name == "id" && p.value == "7"));
    assert!(col
        .warnings
        .iter()
        .any(|w| w.starts_with("1 item(s) with pre-request")));
    assert!(col
        .warnings
        .iter()
        .any(|w| w.contains("Nested") && w.ends_with(": obj")));

    let env = r#"
type: environment.insomnia.rest/5.0
name: Globals
environments:
  name: Base
  data:
    host: h
  subEnvironments:
    - name: Vault
      kvPairData:
        - name: kept
          value: "1"
          enabled: true
        - name: muted
          value: "0"
          enabled: false
"#;
    assert_eq!(detect_format(env), Some(ImportFormat::Insomnia));
    let col = parse(ImportFormat::Insomnia, env).unwrap();
    assert!(col.items.is_empty());
    assert!(col
        .variables
        .iter()
        .any(|v| v.key == "host" && v.value == "h"));
    let vars = &col.environments[0].variables;
    assert!(vars.iter().any(|v| v.key == "kept" && v.enabled));
    assert!(vars.iter().any(|v| v.key == "muted" && !v.enabled));
}
