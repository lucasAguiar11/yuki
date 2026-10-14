mod common;

use common::{bundle, folder, pairs, request};
use yuki_export::to_insomnia;
use yuki_import::{parse, ImportFormat, ImportedAuth, ImportedBody, RawKind};

#[test]
fn insomnia_round_trips_through_the_parser() {
    let out = to_insomnia(&[bundle()]).unwrap();
    assert!(out.content.contains("{{ _.base_url }}"));
    assert!(out.warnings.iter().any(|w| w.contains("WebSocket")));

    let col = parse(ImportFormat::Insomnia, &out.content).unwrap();
    assert_eq!(col.name, "Demo API");
    assert_eq!(col.variables.len(), 1);
    assert_eq!(
        (
            col.variables[0].key.as_str(),
            col.variables[0].value.as_str()
        ),
        ("base_url", "https://api.example.com")
    );
    assert_eq!(col.environments.len(), 1);
    assert_eq!(col.environments[0].name, "Dev");
    assert_eq!(col.environments[0].variables[0].value, "dev-token");

    let users = folder(&col.items, "Users");
    assert!(matches!(&users.auth, ImportedAuth::Bearer { token } if token == "{{ token }}"));

    let create = request(&users.items, "Create user");
    assert_eq!(create.method, "POST");
    assert_eq!(create.url, "{{ base_url }}/users/:id");
    assert_eq!(pairs(&create.query), [("verbose", "true")]);
    // Workspace headers have no Insomnia scope, so they're merged into requests;
    // Content-Type is implied by the JSON body (yuki-http adds it at send time).
    assert_eq!(
        pairs(&create.headers),
        [
            ("Accept", "application/json"),
            ("X-Trace", "{{ trace }}"),
            ("X-Workspace", "demo"),
            ("Content-Type", "application/json")
        ]
    );
    assert!(matches!(
        &create.body,
        Some(ImportedBody::Raw { hint: RawKind::Json, text }) if text == r#"{"name":"{{ name }}"}"#
    ));
    assert!(matches!(
        &create.auth,
        ImportedAuth::Basic { username, password } if username == "ada" && password == "{{ pw }}"
    ));

    // Inherited auth is written explicitly (flattened).
    let list = request(&users.items, "List users");
    assert_eq!(list.method, "GET");
    assert!(matches!(&list.auth, ImportedAuth::Bearer { token } if token == "{{ token }}"));
    assert_eq!(
        pairs(&list.headers),
        [("X-Workspace", "demo")],
        "no body → no Content-Type"
    );
}

#[test]
fn insomnia_combines_bundles_one_group_each() {
    let mut second = bundle();
    second.workspace.id = "ws00000002".into();
    second.workspace.name = "Other API".into();
    let out = to_insomnia(&[bundle(), second]).unwrap();
    let col = parse(ImportFormat::Insomnia, &out.content).unwrap();
    assert_eq!(col.name, "Yuki Export");
    folder(&folder(&col.items, "Demo API").items, "Users");
    folder(&col.items, "Other API");
    assert!(col.environments.iter().any(|e| e.name == "Other API / Dev"));
}

#[test]
fn insomnia_warns_about_disabled_vars_ntlm_workstation_and_content_type_overrides() {
    let mut b = bundle();
    b.environments[1].variables[0].enabled = false;
    b.requests[1].auth = yuki_core::AuthConfig::Ntlm {
        username: "u".into(),
        password: "p".into(),
        password_encrypted: false,
        domain: String::new(),
        workstation: "box".into(),
        enabled: true,
    };
    b.requests[1].body.as_mut().unwrap().content_type = Some("application/vnd.api+json".into());
    let out = to_insomnia(&[b]).unwrap();
    assert!(out
        .warnings
        .iter()
        .any(|w| w.starts_with("1 disabled variable(s)")));
    assert!(out.warnings.iter().any(|w| w.starts_with("1 NTLM")));
    assert!(out
        .warnings
        .iter()
        .any(|w| w.starts_with("1 request(s) carry a Content-Type")));
}
