mod common;

use common::{bundle, folder, pairs, request};
use serde_json::json;
use yuki_core::AuthConfig;
use yuki_export::to_yaak;
use yuki_import::{parse, ImportFormat, ImportedAuth, ImportedBody, RawKind};

#[test]
fn yaak_round_trips_through_the_parser() {
    let out = to_yaak(&[bundle()]).unwrap();
    assert!(out.content.contains("${[ base_url ]}"));
    assert!(out.warnings.iter().any(|w| w.contains("WebSocket")));

    let col = parse(ImportFormat::Yaak, &out.content).unwrap();
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
    // Sibling order follows `order`.
    assert!(
        matches!(&users.items[..], [yuki_import::ImportedItem::Request(a), _] if a.name == "Create user")
    );

    let create = request(&users.items, "Create user");
    assert_eq!(create.method, "POST");
    assert_eq!(create.url, "{{ base_url }}/users/:id");
    assert_eq!(pairs(&create.path_params), [("id", "7")]);
    assert_eq!(pairs(&create.query), [("verbose", "true")]);
    // Content-Type is implied by the JSON body (yuki-http adds it at send time).
    assert_eq!(
        pairs(&create.headers),
        [
            ("Accept", "application/json"),
            ("X-Trace", "{{ trace }}"),
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

    let list = request(&users.items, "List users");
    assert_eq!(list.method, "GET");
    assert!(matches!(list.auth, ImportedAuth::Inherit));
    assert!(list.headers.is_empty(), "no body → no Content-Type");
}

#[test]
fn yaak_combines_bundles_one_folder_each() {
    let mut second = bundle();
    second.workspace.id = "ws00000002".into();
    second.workspace.name = "Other API".into();
    let out = to_yaak(&[bundle(), second]).unwrap();
    let col = parse(ImportFormat::Yaak, &out.content).unwrap();
    assert_eq!(col.name, "Yuki Export");
    folder(&folder(&col.items, "Demo API").items, "Users");
    folder(&col.items, "Other API");
    assert!(col.environments.iter().any(|e| e.name == "Other API / Dev"));
}

#[test]
fn yaak_round_trips_every_auth_scheme() {
    let schemes = [
        json!({ "kind": "oauth2", "grant_type": "authorization_code", "auth_url": "https://a",
            "token_url": "https://t", "client_id": "cid", "client_secret": "cs", "scope": "s",
            "audience": "aud", "client_auth": "request_body", "use_pkce": true,
            "code_challenge_method": "plain", "code_verifier": "ver", "redirect_uri": "http://cb",
            "state": "st", "use_external_browser": true }),
        json!({ "kind": "oauth2", "grant_type": "password", "token_url": "https://t",
            "client_id": "cid", "username": "u", "password": "p", "use_pkce": false }),
        json!({ "kind": "oauth1", "consumer_key": "ck", "consumer_secret": "cs", "token": "t",
            "token_secret": "ts", "signature_method": "rsa_sha256", "private_key": "pk",
            "realm": "r", "callback": "cb", "verifier": "v", "timestamp": "1", "nonce": "n",
            "version": "1.0" }),
        json!({ "kind": "aws_sig_v4", "access_key": "AK", "secret_key": "SK",
            "session_token": "ST", "region": "us-east-1", "service": "s3" }),
        json!({ "kind": "digest", "username": "u", "password": "p" }),
        json!({ "kind": "ntlm", "username": "u", "password": "p", "domain": "D", "workstation": "W" }),
    ];
    for scheme in schemes {
        let auth: AuthConfig = serde_json::from_value(scheme).unwrap();
        let mut b = bundle();
        b.requests[0].auth = auth.clone();
        let name = b.requests[0].name.clone();
        let out = to_yaak(&[b]).unwrap();
        let p = yuki_import::plan(ImportFormat::Yaak, &out.content, None, "ws", 0.0).unwrap();
        let back = &p.requests.iter().find(|r| r.name == name).unwrap().auth;
        assert_eq!(back, &auth);
    }
}

#[test]
fn yaak_warns_about_oauth1_query_params_and_content_type_overrides() {
    let mut b = bundle();
    b.folders[0].auth = serde_json::from_value(json!({
        "kind": "oauth1", "consumer_key": "ck", "consumer_secret": "cs", "params_location": "query",
    }))
    .unwrap();
    b.requests[1].body.as_mut().unwrap().content_type = Some("application/vnd.api+json".into());
    let out = to_yaak(&[b]).unwrap();
    assert!(out.warnings.iter().any(|w| w.starts_with("1 OAuth1")));
    assert!(out
        .warnings
        .iter()
        .any(|w| w.starts_with("1 request(s) carry a Content-Type")));
}
