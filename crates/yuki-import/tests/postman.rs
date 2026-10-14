//! Fixture-driven tests for the Postman v2.1 parser → IR mapping.

use yuki_core::{
    AuthConfig, OAuth1Location, OAuth1Signature, OAuth2ClientAuth, OAuth2Grant, OAuth2PkceMethod,
};
use yuki_import::{
    detect_format, parse, ImportFormat, ImportedAuth, ImportedBody, ImportedItem, RawKind,
};

const SAMPLE: &str = include_str!("fixtures/postman/sample.postman_collection.json");

fn sample() -> yuki_import::ImportedCollection {
    parse(ImportFormat::Postman, SAMPLE).unwrap()
}

#[test]
fn detects_postman() {
    assert_eq!(detect_format(SAMPLE), Some(ImportFormat::Postman));
}

#[test]
fn maps_name_version_variables_and_root_auth() {
    let col = sample();
    assert_eq!(col.name, "Sample API");
    assert_eq!(col.version.as_deref(), Some("2.1"));
    let base = col.variables.iter().find(|v| v.key == "base_url").unwrap();
    assert_eq!(base.value, "https://api.example.com");
    // Collection-level bearer → root auth; the `{{token}}` ref passes through.
    let ImportedAuth::Bearer { token } = &col.root_auth else {
        panic!("expected bearer root auth, got {:?}", col.root_auth);
    };
    assert_eq!(token, "{{token}}");
}

#[test]
fn nests_folders_arbitrary_depth() {
    let col = sample();
    // Top level: folder "Users" + two loose requests.
    let folders: Vec<&str> = col
        .items
        .iter()
        .filter_map(|i| match i {
            ImportedItem::Folder(f) => Some(f.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(folders, vec!["Users"]);

    let users = col
        .items
        .iter()
        .find_map(|i| match i {
            ImportedItem::Folder(f) if f.name == "Users" => Some(f),
            _ => None,
        })
        .unwrap();
    assert_eq!(users.items.len(), 2);
    // No explicit folder auth → inherit.
    assert!(matches!(users.auth, ImportedAuth::Inherit));
}

#[test]
fn parses_object_url_query_path_vars_and_headers() {
    let col = sample();
    let get = all_requests(&col)
        .into_iter()
        .find(|r| r.name == "Get user")
        .unwrap();
    // Query stripped from the stored URL; `:id` template preserved.
    assert_eq!(get.url, "{{base_url}}/users/:id");
    assert!(get.query.iter().any(|p| p.name == "expand" && p.enabled));
    assert!(get.query.iter().any(|p| p.name == "fields" && !p.enabled));
    assert!(get.path_params.iter().any(|p| p.name == "id"));
    assert!(get.headers.iter().any(|h| h.name == "Accept" && h.enabled));
    assert!(get
        .headers
        .iter()
        .any(|h| h.name == "X-Debug" && !h.enabled));
    // No own auth → inherits the collection bearer.
    assert!(matches!(get.auth, ImportedAuth::Inherit));
}

#[test]
fn maps_raw_graphql_and_formdata_bodies() {
    let col = sample();
    let reqs = all_requests(&col);

    let create = reqs.iter().find(|r| r.name == "Create user").unwrap();
    let Some(ImportedBody::Raw {
        hint: RawKind::Json,
        text,
    }) = &create.body
    else {
        panic!("expected raw json body, got {:?}", create.body);
    };
    assert!(text.contains("Ada"));

    let upload = reqs.iter().find(|r| r.name == "Upload avatar").unwrap();
    let Some(ImportedBody::Multipart(fields)) = &upload.body else {
        panic!("expected multipart, got {:?}", upload.body);
    };
    assert!(fields.iter().any(|f| f.name == "file" && f.is_file));
    assert!(fields.iter().any(|f| f.name == "caption" && !f.is_file));
    // Explicit basic auth overrides the collection bearer.
    assert!(matches!(upload.auth, ImportedAuth::Basic { .. }));

    let gql = reqs.iter().find(|r| r.name == "GraphQL search").unwrap();
    let Some(ImportedBody::GraphQl { query, .. }) = &gql.body else {
        panic!("expected graphql, got {:?}", gql.body);
    };
    assert!(query.contains("me"));
}

#[test]
fn reconstructs_url_from_host_path_when_raw_absent() {
    // Some exports omit `url.raw` and carry only host/path arrays.
    let spec = r#"{"info":{"name":"x","schema":"v2.1.0"},"item":[
      {"name":"r","request":{"method":"GET","url":{
        "host":["{{base_url}}"],"path":["users",":id"]}}}]}"#;
    let col = parse(ImportFormat::Postman, spec).unwrap();
    let r = all_requests(&col).into_iter().next().unwrap();
    assert_eq!(r.url, "{{base_url}}/users/:id");
}

#[test]
fn warns_about_uniported_scripts() {
    let col = sample();
    assert!(col.warnings.iter().any(|w| w.contains("script")));
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

fn col(body: &str) -> String {
    format!(
        r#"{{"info":{{"name":"T","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"}},{body}}}"#
    )
}

fn planned(json: &str) -> Vec<(String, AuthConfig)> {
    yuki_import::plan(ImportFormat::Postman, json, None, "ws", 0.0)
        .unwrap()
        .requests
        .into_iter()
        .map(|r| (r.name, r.auth))
        .collect()
}

#[test]
fn folder_auth_is_inherited_without_root_auth_and_noauth_or_unsupported_stop_it() {
    let json = col(
        r#""auth":{"type":"basic","basic":[{"key":"username","value":"root"}]},"item":[
        {"name":"K","auth":{"type":"apikey","apikey":[{"key":"key","value":"X-K"},{"key":"value","value":"v"}]},"item":[
            {"name":"inherits","request":{"url":"http://a"}},
            {"name":"explicit none","request":{"url":"http://a","auth":{"type":"noauth"}}}]},
        {"name":"H","auth":{"type":"hawk","hawk":[]},"item":[{"name":"under hawk","request":{"url":"http://a"}}]},
        {"name":"N","auth":{"type":"noauth"},"item":[{"name":"under noauth","request":{"url":"http://a"}}]},
        {"name":"root inherit","request":{"url":"http://a"}}]"#,
    );
    let got = planned(&json);
    assert!(matches!(&got[0].1, AuthConfig::ApiKey { key, .. } if key == "X-K"));
    assert_eq!(got[1].1, AuthConfig::None);
    // Unsupported folder auth must not fall back to the collection's basic.
    assert_eq!(got[2].1, AuthConfig::None);
    assert_eq!(got[3].1, AuthConfig::None);
    assert!(matches!(&got[4].1, AuthConfig::Basic { username, .. } if username == "root"));

    // No root auth at all: folder auth still reaches the children.
    let json = col(
        r#""item":[{"name":"F","auth":{"type":"bearer","bearer":[{"key":"token","value":"t"}]},
        "item":[{"name":"r","request":{"url":"http://a"}}]}]"#,
    );
    assert!(matches!(&planned(&json)[0].1, AuthConfig::Bearer { token, .. } if token == "t"));
}

#[test]
fn reads_v20_object_shaped_auth() {
    let json = r#"{"info":{"name":"T","schema":"https://schema.getpostman.com/json/collection/v2.0.0/collection.json"},"item":[
        {"name":"b","request":{"url":"http://a","auth":{"type":"bearer","bearer":{"token":"tok"}}}},
        {"name":"u","request":{"url":"http://a","auth":{"type":"basic","basic":{"username":"u","password":"p"}}}}]}"#;
    let reqs = all_requests(&parse(ImportFormat::Postman, json).unwrap());
    assert!(matches!(&reqs[0].auth, ImportedAuth::Bearer { token } if token == "tok"));
    assert!(
        matches!(&reqs[1].auth, ImportedAuth::Basic { username, password } if username == "u" && password == "p")
    );
}

#[test]
fn maps_oauth1_aws_digest_ntlm_to_core_and_flags_others() {
    let json = col(r#""item":[
        {"name":"o1","request":{"url":"http://a","auth":{"type":"oauth1","oauth1":[{"key":"consumerKey","value":"ck"},{"key":"signatureMethod","value":"RSA-SHA256"},{"key":"privateKey","value":"PEM"},{"key":"addParamsToHeader","value":"false"},{"key":"nonce","value":"n"}]}}},
        {"name":"o1b","request":{"url":"http://a","auth":{"type":"oauth1","oauth1":{"consumerKey":"ck","addParamsToHeader":false,"timestamp":1453890475}}}},
        {"name":"aws","request":{"url":"http://a","auth":{"type":"awsv4","awsv4":[{"key":"accessKey","value":"AK"},{"key":"region","value":"eu-west-1"},{"key":"service","value":"s3"},{"key":"sessionToken","value":"st"}]}}},
        {"name":"dg","request":{"url":"http://a","auth":{"type":"digest","digest":[{"key":"username","value":"u"},{"key":"password","value":"p"}]}}},
        {"name":"nt","request":{"url":"http://a","auth":{"type":"ntlm","ntlm":[{"key":"username","value":"u"},{"key":"domain","value":"CORP"}]}}},
        {"name":"hk","request":{"url":"http://a","auth":{"type":"hawk","hawk":[]}}}]"#);
    let reqs = all_requests(&parse(ImportFormat::Postman, &json).unwrap());
    let core = |i: usize| match &reqs[i].auth {
        ImportedAuth::Core(a) => a.clone(),
        other => panic!("expected core auth, got {other:?}"),
    };
    assert!(
        matches!(core(0), AuthConfig::OAuth1 { consumer_key, signature_method: OAuth1Signature::RsaSha256, private_key, params_location: OAuth1Location::Query, nonce, .. }
        if consumer_key == "ck" && private_key == "PEM" && nonce == "n")
    );
    assert!(
        matches!(core(1), AuthConfig::OAuth1 { params_location: OAuth1Location::Query, timestamp, .. } if timestamp == "1453890475")
    );
    assert!(
        matches!(core(2), AuthConfig::AwsSigV4 { access_key, region, service, session_token, .. }
        if access_key == "AK" && region == "eu-west-1" && service == "s3" && session_token == "st")
    );
    assert!(
        matches!(core(3), AuthConfig::Digest { username, password, .. } if username == "u" && password == "p")
    );
    assert!(
        matches!(core(4), AuthConfig::Ntlm { username, domain, .. } if username == "u" && domain == "CORP")
    );
    assert!(matches!(&reqs[5].auth, ImportedAuth::Unsupported(k) if k == "hawk"));
}

#[test]
fn maps_full_oauth2() {
    let json = col(r#""item":[
        {"name":"pkce","request":{"url":"http://a","auth":{"type":"oauth2","oauth2":[{"key":"grant_type","value":"authorization_code_with_pkce"},{"key":"challengeAlgorithm","value":"plain"},{"key":"code_verifier","value":"cv"},{"key":"redirect_uri","value":"http://cb"},{"key":"state","value":"st"},{"key":"audience","value":"aud"},{"key":"client_authentication","value":"body"}]}}},
        {"name":"code","request":{"url":"http://a","auth":{"type":"oauth2","oauth2":[{"key":"grant_type","value":"authorization_code"}]}}},
        {"name":"pw","request":{"url":"http://a","auth":{"type":"oauth2","oauth2":[{"key":"grant_type","value":"password_credentials"},{"key":"username","value":"u"},{"key":"password","value":"p"}]}}}]"#);
    let auths: Vec<AuthConfig> = planned(&json).into_iter().map(|(_, a)| a).collect();
    assert!(
        matches!(&auths[0], AuthConfig::OAuth2 { grant_type: OAuth2Grant::AuthorizationCode, use_pkce: true, code_challenge_method: OAuth2PkceMethod::Plain, code_verifier, redirect_uri, state, audience, client_auth: OAuth2ClientAuth::RequestBody, .. }
        if code_verifier == "cv" && redirect_uri == "http://cb" && state == "st" && audience == "aud")
    );
    assert!(matches!(
        &auths[1],
        AuthConfig::OAuth2 {
            grant_type: OAuth2Grant::AuthorizationCode,
            use_pkce: false,
            ..
        }
    ));
    assert!(
        matches!(&auths[2], AuthConfig::OAuth2 { grant_type: OAuth2Grant::Password, username, password, .. } if username == "u" && password == "p")
    );
}

#[test]
fn parses_raw_only_query_file_paths_and_variables() {
    let json = col(
        r#""variable":[{"key":"n","value":5},{"key":"off","value":"x","disabled":true}],"item":[
        {"name":"q","request":{"url":{"raw":"{{BASE}}/get?a=1&b={{v}}&flag","host":["{{BASE}}"],"path":["get"]}}},
        {"name":"fd","request":{"url":"http://a","body":{"mode":"formdata","formdata":[{"key":"f","type":"file","src":["/tmp/a.png","/tmp/b.png"]}]}}},
        {"name":"bin","request":{"url":"http://a","body":{"mode":"file","file":{"src":"/tmp/c.bin"}}}},
        {"name":"dyn","request":{"url":"http://a?id={{$guid}}&t={{$timestamp}}"}}]"#,
    );
    let c = parse(ImportFormat::Postman, &json).unwrap();
    let reqs = all_requests(&c);
    assert_eq!(reqs[0].url, "{{BASE}}/get");
    let q: Vec<_> = reqs[0]
        .query
        .iter()
        .map(|p| (p.name.as_str(), p.value.as_str()))
        .collect();
    assert_eq!(q, [("a", "1"), ("b", "{{v}}"), ("flag", "")]);
    let Some(ImportedBody::Multipart(f)) = &reqs[1].body else {
        panic!()
    };
    assert!(f[0].is_file && f[0].value == "/tmp/a.png");
    assert!(matches!(&reqs[2].body, Some(ImportedBody::Binary(p)) if p == "/tmp/c.bin"));
    assert_eq!(
        (c.variables[0].value.as_str(), c.variables[0].enabled),
        ("5", true)
    );
    assert!(!c.variables[1].enabled);
    assert!(c
        .warnings
        .iter()
        .any(|w| w.starts_with("2 Postman dynamic variable")));
}

#[test]
fn imports_environment_and_globals_files() {
    let env = r#"{"name":"dev","values":[{"key":"base","value":"http://x","type":"default","enabled":true},{"key":"off","value":"1","enabled":false}],"_postman_variable_scope":"environment"}"#;
    assert_eq!(detect_format(env), Some(ImportFormat::Postman));
    let c = parse(ImportFormat::Postman, env).unwrap();
    assert!(c.items.is_empty() && c.variables.is_empty());
    assert_eq!(c.environments[0].name, "dev");
    let vars = &c.environments[0].variables;
    assert_eq!(
        (
            vars[0].key.as_str(),
            vars[0].value.as_str(),
            vars[0].enabled
        ),
        ("base", "http://x", true)
    );
    assert!(!vars[1].enabled);

    let globals = r#"{"name":"g","values":[{"key":"k","value":"v","enabled":true}],"_postman_variable_scope":"globals"}"#;
    let c = parse(ImportFormat::Postman, globals).unwrap();
    assert!(c.environments.is_empty());
    assert_eq!(c.variables[0].key, "k");
}
