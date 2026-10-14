//! Postman `auth` → IR. v2.1 stores a scheme's fields as `{type, <type>:
//! [{key,value}]}`; v2.0 as `{type, <type>: {field: value}}`. Both are read.

use super::scalar;
use crate::ir::ImportedAuth;
use serde_json::Value;
use yuki_core::{
    AuthConfig, OAuth1Location, OAuth1Signature, OAuth2ClientAuth, OAuth2Grant, OAuth2PkceMethod,
};

/// `None`/`null` = no `auth` key = inherit the enclosing scope.
pub(super) fn to_auth(auth: Option<&Value>) -> ImportedAuth {
    let Some(auth) = auth.filter(|a| !a.is_null()) else {
        return ImportedAuth::Inherit;
    };
    let ty = auth.get("type").and_then(Value::as_str).unwrap_or("noauth");
    let params = auth.get(ty);
    let get = |k: &str| -> String {
        let v = match params {
            Some(Value::Array(a)) => a
                .iter()
                .find(|e| e.get("key").and_then(Value::as_str) == Some(k))
                .and_then(|e| e.get("value")),
            Some(o) => o.get(k),
            None => None,
        };
        v.map(scalar).unwrap_or_default()
    };
    match ty {
        "noauth" => ImportedAuth::None,
        "bearer" => ImportedAuth::Bearer {
            token: get("token"),
        },
        "basic" => ImportedAuth::Basic {
            username: get("username"),
            password: get("password"),
        },
        "apikey" => {
            let key = get("key");
            ImportedAuth::ApiKey {
                key: if key.is_empty() {
                    "X-API-Key".into()
                } else {
                    key
                },
                value: get("value"),
                in_header: get("in") != "query",
            }
        }
        "oauth2" => ImportedAuth::Core(oauth2(&get)),
        "oauth1" => ImportedAuth::Core(AuthConfig::OAuth1 {
            consumer_key: get("consumerKey"),
            consumer_secret: get("consumerSecret"),
            consumer_secret_encrypted: false,
            token: get("token"),
            token_secret: get("tokenSecret"),
            token_secret_encrypted: false,
            signature_method: match get("signatureMethod").as_str() {
                "HMAC-SHA256" => OAuth1Signature::HmacSha256,
                "HMAC-SHA512" => OAuth1Signature::HmacSha512,
                "RSA-SHA1" => OAuth1Signature::RsaSha1,
                "RSA-SHA256" => OAuth1Signature::RsaSha256,
                "RSA-SHA512" => OAuth1Signature::RsaSha512,
                "PLAINTEXT" => OAuth1Signature::PlainText,
                _ => OAuth1Signature::HmacSha1,
            },
            realm: get("realm"),
            private_key: get("privateKey"),
            private_key_encrypted: false,
            // Bool in Postman's own files, "true"/"false" string in older/Yuki exports.
            params_location: if get("addParamsToHeader") == "false" {
                OAuth1Location::Query
            } else {
                OAuth1Location::Header
            },
            callback: get("callback"),
            verifier: get("verifier"),
            timestamp: get("timestamp"),
            nonce: get("nonce"),
            version: get("version"),
            enabled: true,
        }),
        "awsv4" => {
            // v2.0 files carry the service as `serviceName`.
            let service = Some(get("service"))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| get("serviceName"));
            ImportedAuth::Core(AuthConfig::AwsSigV4 {
                access_key: get("accessKey"),
                secret_key: get("secretKey"),
                secret_key_encrypted: false,
                session_token: get("sessionToken"),
                session_token_encrypted: false,
                region: get("region"),
                service,
                enabled: true,
            })
        }
        "digest" => ImportedAuth::Core(AuthConfig::Digest {
            username: get("username"),
            password: get("password"),
            password_encrypted: false,
            enabled: true,
        }),
        "ntlm" => ImportedAuth::Core(AuthConfig::Ntlm {
            username: get("username"),
            password: get("password"),
            password_encrypted: false,
            domain: get("domain"),
            workstation: get("workstation"),
            enabled: true,
        }),
        other => ImportedAuth::Unsupported(other.to_string()),
    }
}

fn oauth2(get: &dyn Fn(&str) -> String) -> AuthConfig {
    let grant = get("grant_type");
    let mut a = crate::convert::oauth2(
        match grant.as_str() {
            "authorization_code" | "authorization_code_with_pkce" => OAuth2Grant::AuthorizationCode,
            "password_credentials" | "password" => OAuth2Grant::Password,
            "implicit" => OAuth2Grant::Implicit,
            _ => OAuth2Grant::ClientCredentials,
        },
        get("authUrl"),
        get("accessTokenUrl"),
        get("clientId"),
        get("clientSecret"),
        get("scope"),
    );
    if let AuthConfig::OAuth2 {
        username,
        password,
        redirect_uri,
        state,
        audience,
        client_auth,
        use_pkce,
        code_challenge_method,
        code_verifier,
        ..
    } = &mut a
    {
        *username = get("username");
        *password = get("password");
        *redirect_uri = get("redirect_uri");
        *state = get("state");
        *audience = get("audience");
        if get("client_authentication") == "body" {
            *client_auth = OAuth2ClientAuth::RequestBody;
        }
        // Postman models PKCE as its own grant; plain auth code means no PKCE.
        if grant == "authorization_code" {
            *use_pkce = false;
        }
        if get("challengeAlgorithm") == "plain" {
            *code_challenge_method = OAuth2PkceMethod::Plain;
        }
        *code_verifier = get("code_verifier");
    }
    a
}
