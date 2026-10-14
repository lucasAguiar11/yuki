//! Insomnia `authentication` (v4 and v5 share the shape, `insomnia-data`
//! `models/request.ts`) → IR auth.

use super::s;
use crate::ir::ImportedAuth;
use serde_json::Value;
use yuki_core::{
    AuthConfig, OAuth1Location, OAuth1Signature, OAuth2ClientAuth, OAuth2Grant, OAuth2PkceMethod,
};

pub(crate) fn to_auth(a: &Value) -> ImportedAuth {
    // Insomnia: `{}` or a disabled scheme falls back to the nearest folder's auth.
    if a.as_object().is_none_or(|o| o.is_empty())
        || a.get("disabled").and_then(Value::as_bool) == Some(true)
    {
        return ImportedAuth::Inherit;
    }
    let t = |k: &str| s(a, k);
    let core = match a.get("type").and_then(Value::as_str).unwrap_or("") {
        "" => return ImportedAuth::Inherit,
        "none" => return ImportedAuth::None,
        "bearer" => {
            return match t("prefix").as_str() {
                "" | "Bearer" => ImportedAuth::Bearer { token: t("token") },
                // Core bearer has no prefix: send the exact header instead.
                prefix => ImportedAuth::ApiKey {
                    key: "Authorization".into(),
                    value: if prefix == "NO_PREFIX" {
                        t("token")
                    } else {
                        format!("{prefix} {}", t("token"))
                    },
                    in_header: true,
                },
            };
        }
        "basic" => {
            return ImportedAuth::Basic {
                username: t("username"),
                password: t("password"),
            }
        }
        "apikey" => {
            let key = Some(t("key"))
                .filter(|k| !k.is_empty())
                .unwrap_or_else(|| "X-API-Key".into());
            return match a.get("addTo").and_then(Value::as_str) {
                // Core has no cookie location: send it as a Cookie header.
                Some("cookie") => ImportedAuth::ApiKey {
                    value: format!("{key}={}", t("value")),
                    key: "Cookie".into(),
                    in_header: true,
                },
                to => ImportedAuth::ApiKey {
                    key,
                    value: t("value"),
                    in_header: to != Some("queryParams"),
                },
            };
        }
        "digest" => AuthConfig::Digest {
            username: t("username"),
            password: t("password"),
            password_encrypted: false,
            enabled: true,
        },
        "ntlm" => {
            // Insomnia has no domain field; `DOMAIN\user` carries it.
            let user = t("username");
            let (domain, username) = user.split_once('\\').unwrap_or(("", &user));
            AuthConfig::Ntlm {
                username: username.into(),
                password: t("password"),
                password_encrypted: false,
                domain: domain.into(),
                workstation: String::new(),
                enabled: true,
            }
        }
        "iam" => AuthConfig::AwsSigV4 {
            access_key: t("accessKeyId"),
            secret_key: t("secretAccessKey"),
            secret_key_encrypted: false,
            session_token: t("sessionToken"),
            session_token_encrypted: false,
            region: t("region"),
            service: t("service"),
            enabled: true,
        },
        "oauth1" => AuthConfig::OAuth1 {
            consumer_key: t("consumerKey"),
            consumer_secret: t("consumerSecret"),
            consumer_secret_encrypted: false,
            token: t("tokenKey"),
            token_secret: t("tokenSecret"),
            token_secret_encrypted: false,
            signature_method: match t("signatureMethod").as_str() {
                "HMAC-SHA256" => OAuth1Signature::HmacSha256,
                "HMAC-SHA512" => OAuth1Signature::HmacSha512,
                "RSA-SHA1" => OAuth1Signature::RsaSha1,
                "RSA-SHA256" => OAuth1Signature::RsaSha256,
                "RSA-SHA512" => OAuth1Signature::RsaSha512,
                "PLAINTEXT" => OAuth1Signature::PlainText,
                _ => OAuth1Signature::HmacSha1,
            },
            realm: t("realm"),
            private_key: t("privateKey"),
            private_key_encrypted: false,
            params_location: OAuth1Location::Header,
            callback: t("callback"),
            verifier: t("verifier"),
            timestamp: t("timestamp"),
            nonce: t("nonce"),
            version: t("version"),
            enabled: true,
        },
        "oauth2" => oauth2(a),
        other => return ImportedAuth::Unsupported(other.to_string()),
    };
    ImportedAuth::Core(core)
}

fn oauth2(a: &Value) -> AuthConfig {
    let t = |k: &str| s(a, k);
    let flag = |k: &str| a.get(k).and_then(Value::as_bool).unwrap_or(false);
    let mut auth = crate::convert::oauth2(
        match t("grantType").as_str() {
            "authorization_code" => OAuth2Grant::AuthorizationCode,
            "password" => OAuth2Grant::Password,
            "implicit" => OAuth2Grant::Implicit,
            _ => OAuth2Grant::ClientCredentials,
        },
        t("authorizationUrl"),
        t("accessTokenUrl"),
        t("clientId"),
        t("clientSecret"),
        t("scope"),
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
        use_external_browser,
        ..
    } = &mut auth
    {
        *username = t("username");
        *password = t("password");
        *redirect_uri = t("redirectUrl");
        *state = t("state");
        *audience = t("audience");
        // Insomnia only runs PKCE when `usePkce` is set.
        *use_pkce = flag("usePkce");
        if t("pkceMethod") == "plain" {
            *code_challenge_method = OAuth2PkceMethod::Plain;
        }
        if flag("credentialsInBody") {
            *client_auth = OAuth2ClientAuth::RequestBody;
        }
        *use_external_browser = flag("useDefaultBrowser");
    }
    auth
}
