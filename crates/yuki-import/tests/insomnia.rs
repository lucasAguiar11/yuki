//! Fixture-driven tests for the Insomnia v4 parser → IR mapping.

use yuki_import::{
    detect_format, parse, ImportFormat, ImportedAuth, ImportedBody, ImportedItem, RawKind,
};

const EXPORT: &str = include_str!("fixtures/insomnia/export.json");
const CYCLIC: &str = include_str!("fixtures/insomnia/cyclic.json");

fn sample() -> yuki_import::ImportedCollection {
    parse(ImportFormat::Insomnia, EXPORT).unwrap()
}

#[test]
fn detects_insomnia() {
    assert_eq!(detect_format(EXPORT), Some(ImportFormat::Insomnia));
}

#[test]
fn maps_name_version_and_base_environment() {
    let col = sample();
    assert_eq!(col.name, "Sample API");
    assert_eq!(col.version.as_deref(), Some("4"));
    let base = col.variables.iter().find(|v| v.key == "base_url").unwrap();
    assert_eq!(base.value, "https://api.example.com");
}

#[test]
fn reconstructs_tree_ordered_by_sort_key() {
    let col = sample();
    // metaSortKey: Users(-100), Upload(-70), GraphQL(-60).
    let names: Vec<String> = col
        .items
        .iter()
        .map(|i| match i {
            ImportedItem::Folder(f) => f.name.clone(),
            ImportedItem::Request(r) => r.name.clone(),
        })
        .collect();
    assert_eq!(names, vec!["Users", "Upload avatar", "GraphQL search"]);

    let users = col
        .items
        .iter()
        .find_map(|i| match i {
            ImportedItem::Folder(f) if f.name == "Users" => Some(f),
            _ => None,
        })
        .unwrap();
    assert_eq!(users.items.len(), 2);
    // Folder bearer auth; `{{ _.token }}` normalized to `{{ token }}`.
    let ImportedAuth::Bearer { token } = &users.auth else {
        panic!("expected bearer folder auth, got {:?}", users.auth);
    };
    assert_eq!(token, "{{ token }}");
}

#[test]
fn normalizes_template_vars_and_classifies_params() {
    let col = sample();
    let get = all_requests(&col)
        .into_iter()
        .find(|r| r.name == "Get user")
        .unwrap();
    assert_eq!(get.url, "{{ base_url }}/users/:id");
    assert!(get.query.iter().any(|p| p.name == "expand" && p.enabled));
    assert!(get.query.iter().any(|p| p.name == "fields" && !p.enabled));
    assert!(get.headers.iter().any(|h| h.name == "Accept"));
    // Empty `authentication: {}` inherits the folder bearer.
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
        panic!("expected raw json, got {:?}", create.body);
    };
    assert!(text.contains("Ada"));

    let upload = reqs.iter().find(|r| r.name == "Upload avatar").unwrap();
    let Some(ImportedBody::Multipart(fields)) = &upload.body else {
        panic!("expected multipart, got {:?}", upload.body);
    };
    assert!(fields.iter().any(|f| f.name == "file" && f.is_file));
    assert!(matches!(upload.auth, ImportedAuth::Basic { .. }));

    let gql = reqs.iter().find(|r| r.name == "GraphQL search").unwrap();
    let Some(ImportedBody::GraphQl { query, .. }) = &gql.body else {
        panic!("expected graphql, got {:?}", gql.body);
    };
    assert!(query.contains("me"));
    // Loose request with empty auth inherits the root (resolves to none).
    assert!(matches!(gql.auth, ImportedAuth::Inherit));
}

#[test]
fn warns_about_unsupported_tags() {
    // The `{% now %}` tag in a header value should surface a warning.
    let col = sample();
    assert!(col.warnings.iter().any(|w| w.contains("template tags")));
}

#[test]
fn warns_about_filter_and_imports_sub_environment() {
    // A `|` filter inside a tag, plus a second (sub-)environment resource.
    let spec = r#"{"_type":"export","__export_format":4,"resources":[
      {"_id":"wrk_1","_type":"workspace","parentId":null,"name":"X"},
      {"_id":"env_base","_type":"environment","parentId":"wrk_1","data":{"k":"v"}},
      {"_id":"env_sub","_type":"environment","parentId":"env_base","name":"Staging","data":{"k":"v2"}},
      {"_id":"r1","_type":"request","parentId":"wrk_1","name":"R","method":"GET",
       "url":"{{ _.base | upper }}/x","authentication":{}}]}"#;
    let col = parse(ImportFormat::Insomnia, spec).unwrap();
    assert!(col.warnings.iter().any(|w| w.contains("filters")));
    // The sub-environment is imported as its own environment (no warning).
    assert!(col.environments.iter().any(|e| e.name == "Staging"));
    // A plain `|` in a JSON value (not inside a tag) must not trip the filter warning.
    let clean = r#"{"_type":"export","__export_format":4,"resources":[
      {"_id":"w","_type":"workspace","parentId":null,"name":"Y"},
      {"_id":"r","_type":"request","parentId":"w","name":"R","method":"GET",
       "url":"https://x/y","headers":[{"name":"X","value":"a|b"}],"authentication":{}}]}"#;
    let col2 = parse(ImportFormat::Insomnia, clean).unwrap();
    assert!(!col2.warnings.iter().any(|w| w.contains("filters")));
}

#[test]
fn cyclic_parent_ids_terminate() {
    // A crafted export with a parentId cycle (duplicate group id looping back)
    // must import without unbounded recursion / stack overflow.
    let col = parse(ImportFormat::Insomnia, CYCLIC).unwrap();
    assert!(!col.items.is_empty());
}

const EDGE: &str = r#"{"_type":"export","__export_format":4,"resources":[
  {"_id":"wrk_1","_type":"workspace","parentId":null,"name":"edge"},
  {"_id":"fld_1","_type":"request_group","parentId":"wrk_1","name":"F",
   "authentication":{"type":"bearer","token":"folderTok"},"preRequestScript":"x()"},
  {"_id":"req_1","_type":"request","parentId":"fld_1","name":"disabled-auth","method":"get",
   "url":"{{ _['api-host'] }}/users/:id","pathParameters":[{"name":"id","value":"42"}],
   "authentication":{"type":"basic","username":"u","password":"p","disabled":true},
   "afterResponseScript":"insomnia.test('x',()=>{})"},
  {"_id":"req_2","_type":"request","parentId":"wrk_1","name":"binary","method":"POST",
   "url":"{{ _[\"base\"] }}/up","body":{"mimeType":"application/octet-stream","fileName":"/tmp/a.bin"},
   "authentication":{"type":"bearer","token":"t","prefix":"Token"}},
  {"_id":"req_3","_type":"request","parentId":"wrk_1","name":"cookie","method":"GET","url":"https://x",
   "authentication":{"type":"apikey","key":"sid","value":"v","addTo":"cookie"}},
  {"_id":"req_4","_type":"request","parentId":"wrk_1","name":"multipart","method":"POST","url":"https://x",
   "body":{"mimeType":"multipart/form-data","params":[{"name":"f","type":"file","fileName":"/tmp/a.png"}]},
   "authentication":{"type":"hawk","id":"i","key":"k","algorithm":"sha256"}},
  {"_id":"greq_1","_type":"grpc_request","parentId":"wrk_1","name":"grpc"},
  {"_id":"ws-req_1","_type":"websocket_request","parentId":"wrk_1","name":"ws"}]}"#;

#[test]
fn maps_edge_cases_and_warns_about_dropped_items() {
    let col = parse(ImportFormat::Insomnia, EDGE).unwrap();
    let reqs = all_requests(&col);
    let get = |n: &str| reqs.iter().find(|r| r.name == n).unwrap();

    let r = get("disabled-auth");
    // Disabled auth falls back to the folder's, like Insomnia.
    assert!(matches!(r.auth, ImportedAuth::Inherit));
    assert_eq!(r.url, "{{ api-host }}/users/:id");
    assert!(r
        .path_params
        .iter()
        .any(|p| p.name == "id" && p.value == "42"));

    let r = get("binary");
    assert_eq!(r.url, "{{ base }}/up");
    assert!(matches!(&r.body, Some(ImportedBody::Binary(p)) if p == "/tmp/a.bin"));
    assert!(
        matches!(&r.auth, ImportedAuth::ApiKey { key, value, in_header: true }
        if key == "Authorization" && value == "Token t")
    );

    assert!(
        matches!(&get("cookie").auth, ImportedAuth::ApiKey { key, value, in_header: true }
        if key == "Cookie" && value == "sid=v")
    );

    let r = get("multipart");
    let Some(ImportedBody::Multipart(fields)) = &r.body else {
        panic!("expected multipart, got {:?}", r.body);
    };
    assert!(fields[0].is_file && fields[0].value == "/tmp/a.png");
    assert!(matches!(&r.auth, ImportedAuth::Unsupported(k) if k == "hawk"));

    assert!(col
        .warnings
        .iter()
        .any(|w| w == "2 WebSocket/gRPC item(s) skipped"));
    assert!(col
        .warnings
        .iter()
        .any(|w| w.starts_with("2 item(s) with pre-request")));
}

#[test]
fn maps_every_core_auth_kind() {
    use yuki_core::{AuthConfig, OAuth1Signature, OAuth2ClientAuth, OAuth2Grant, OAuth2PkceMethod};
    let spec = r#"{"_type":"export","__export_format":4,"resources":[
      {"_id":"w","_type":"workspace","parentId":null,"name":"A"},
      {"_id":"r1","_type":"request","parentId":"w","name":"digest","method":"GET","url":"u",
       "authentication":{"type":"digest","username":"du","password":"dp"}},
      {"_id":"r2","_type":"request","parentId":"w","name":"ntlm","method":"GET","url":"u",
       "authentication":{"type":"ntlm","username":"CORP\\nu","password":"np"}},
      {"_id":"r3","_type":"request","parentId":"w","name":"oauth1","method":"GET","url":"u",
       "authentication":{"type":"oauth1","consumerKey":"ck","consumerSecret":"cs","tokenKey":"tk",
        "tokenSecret":"ts","signatureMethod":"RSA-SHA1","privateKey":"pk","realm":"r","verifier":"v"}},
      {"_id":"r4","_type":"request","parentId":"w","name":"iam","method":"GET","url":"u",
       "authentication":{"type":"iam","accessKeyId":"ak","secretAccessKey":"sk","sessionToken":"st",
        "region":"us-east-1","service":"s3"}},
      {"_id":"r5","_type":"request","parentId":"w","name":"oauth2","method":"GET","url":"u",
       "authentication":{"type":"oauth2","grantType":"password","accessTokenUrl":"t","clientId":"c",
        "username":"ou","password":"op","redirectUrl":"cb","audience":"aud","state":"st",
        "usePkce":false,"pkceMethod":"plain","credentialsInBody":true,"useDefaultBrowser":true}}]}"#;
    let col = parse(ImportFormat::Insomnia, spec).unwrap();
    let reqs = all_requests(&col);
    let core = |n: &str| match &reqs.iter().find(|r| r.name == n).unwrap().auth {
        ImportedAuth::Core(a) => a.clone(),
        other => panic!("{n}: expected core auth, got {other:?}"),
    };
    assert!(
        matches!(core("digest"), AuthConfig::Digest { username, password, .. }
        if username == "du" && password == "dp")
    );
    assert!(
        matches!(core("ntlm"), AuthConfig::Ntlm { username, domain, .. }
        if username == "nu" && domain == "CORP")
    );
    assert!(matches!(core("oauth1"), AuthConfig::OAuth1 {
        consumer_secret, token, signature_method: OAuth1Signature::RsaSha1, private_key, verifier, ..
    } if consumer_secret == "cs" && token == "tk" && private_key == "pk" && verifier == "v"));
    assert!(
        matches!(core("iam"), AuthConfig::AwsSigV4 { session_token, region, service, .. }
        if session_token == "st" && region == "us-east-1" && service == "s3")
    );
    assert!(matches!(core("oauth2"), AuthConfig::OAuth2 {
        grant_type: OAuth2Grant::Password, username, password, redirect_uri, audience, state,
        use_pkce: false, code_challenge_method: OAuth2PkceMethod::Plain,
        client_auth: OAuth2ClientAuth::RequestBody, use_external_browser: true, ..
    } if username == "ou" && password == "op" && redirect_uri == "cb" && audience == "aud" && state == "st"));
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
