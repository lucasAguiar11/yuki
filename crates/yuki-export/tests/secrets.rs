mod common;

use common::{bundle, var};
use yuki_core::{
    ApiFolder, ApiKeyLocation, AuthConfig, Environment, GrpcRequest, HttpRequest, OAuth1Location,
    OAuth2Grant, ProtoSource, Workspace, WsConnection,
};

type Snapshot = (
    Workspace,
    Vec<ApiFolder>,
    Vec<HttpRequest>,
    Vec<WsConnection>,
    Vec<GrpcRequest>,
    Vec<Environment>,
);

fn s(v: &str) -> String {
    v.to_string()
}

/// One instance of every scheme with a secret, all secrets set to "SECRET".
fn every_secret_kind() -> Vec<AuthConfig> {
    let oauth2: AuthConfig = serde_json::from_value(serde_json::json!({
        "kind": "oauth2", "grant_type": "password", "token_url": "https://t", "client_id": "cid",
        "client_secret": "SECRET", "username": "u", "password": "SECRET",
    }))
    .unwrap();
    let oauth1: AuthConfig = serde_json::from_value(serde_json::json!({
        "kind": "oauth1", "consumer_key": "ck", "consumer_secret": "SECRET", "token": "tok",
        "token_secret": "SECRET", "private_key": "SECRET",
    }))
    .unwrap();
    assert!(matches!(
        oauth1,
        AuthConfig::OAuth1 {
            params_location: OAuth1Location::Header,
            ..
        }
    ));
    assert!(matches!(
        oauth2,
        AuthConfig::OAuth2 {
            grant_type: OAuth2Grant::Password,
            ..
        }
    ));
    vec![
        AuthConfig::Bearer {
            token: s("SECRET"),
            token_encrypted: true,
            enabled: true,
        },
        AuthConfig::Basic {
            username: s("u"),
            password: s("SECRET"),
            password_encrypted: true,
            enabled: true,
        },
        AuthConfig::ApiKey {
            key: s("X-Key"),
            value: s("SECRET"),
            location: ApiKeyLocation::Header,
            value_encrypted: true,
            enabled: true,
        },
        AuthConfig::AwsSigV4 {
            access_key: s("AKIA"),
            secret_key: s("SECRET"),
            secret_key_encrypted: true,
            session_token: s("SECRET"),
            session_token_encrypted: true,
            region: s("us-east-1"),
            service: s("s3"),
            enabled: true,
        },
        oauth1,
        oauth2,
        AuthConfig::Digest {
            username: s("u"),
            password: s("SECRET"),
            password_encrypted: true,
            enabled: true,
        },
        AuthConfig::Ntlm {
            username: s("u"),
            password: s("SECRET"),
            password_encrypted: true,
            domain: s("CORP"),
            workstation: s("ws"),
            enabled: true,
        },
    ]
}

#[test]
fn strip_secrets_blanks_every_secret_and_nothing_else() {
    for auth in every_secret_kind() {
        let mut b = bundle();
        b.workspace.auth = auth.clone();
        b.folders[0].auth = auth.clone();
        b.folders[0].variables = vec![
            var("folder_secret", "SECRET", true),
            var("plain", "keep", false),
        ];
        for r in &mut b.requests {
            r.auth = auth.clone();
            r.variations = vec![yuki_core::Variation {
                id: s("v1"),
                name: s("admin"),
                auth: Some(auth.clone()),
                ..Default::default()
            }];
        }
        b.ws[0].auth = auth.clone();
        b.grpc.push(GrpcRequest {
            id: s("g1"),
            request_type: s("grpc"),
            model: s("grpc_request"),
            workspace_id: s("ws00000001"),
            folder_id: None,
            name: s("Call"),
            target: s("localhost:50051"),
            tls: false,
            proto_source: ProtoSource::Reflection,
            service: None,
            method: None,
            metadata: vec![],
            message: s("{}"),
            auth: auth.clone(),
            order: 0.0,
            created_at: common::ts(),
            updated_at: common::ts(),
        });
        let before = serde_json::to_string(&(
            &b.workspace,
            &b.folders,
            &b.requests,
            &b.ws,
            &b.grpc,
            &b.environments,
        ))
        .unwrap();

        b.strip_secrets();

        let after = serde_json::to_string(&(
            &b.workspace,
            &b.folders,
            &b.requests,
            &b.ws,
            &b.grpc,
            &b.environments,
        ))
        .unwrap();
        assert!(!after.contains("SECRET"), "{after}");
        // Everything else is untouched: blanking the secrets in `before` yields
        // `after` (re-serialized so newly-empty skip-if-empty fields drop out alike).
        let expected: Snapshot =
            serde_json::from_str(&before.replace("SECRET", "").replace("dev-token", "")).unwrap();
        assert_eq!(serde_json::to_string(&expected).unwrap(), after);
        assert!(
            !after.contains("dev-token"),
            "encrypted env var kept: {after}"
        );
        assert!(after.contains("https://api.example.com") && after.contains("keep"));
    }
}
