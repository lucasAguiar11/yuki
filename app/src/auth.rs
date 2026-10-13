//! Auth schemes as data: which fields each `AuthConfig` variant shows, and the
//! form ⇄ model mapping. Conversion goes through the serde shape (`kind` tag +
//! snake_case fields) so every field the form doesn't show (`*_encrypted`,
//! `enabled`, OAuth 1 advanced overrides…) survives an edit untouched.

use serde_json::{Map, Value};
use yuki_core::AuthConfig;

#[derive(Clone, Copy, PartialEq)]
pub enum Ty {
    Text,
    /// Masked input; encrypted at rest in encrypted workspaces.
    Secret,
    /// One of these serde values (first = default).
    Choice(&'static [&'static str]),
    /// Boolean with its serde default.
    Flag(bool),
}

#[derive(Clone, Copy)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub ty: Ty,
    /// OAuth 2 grants this field applies to; empty = always shown.
    pub grants: &'static [&'static str],
}

const fn f(key: &'static str, label: &'static str, ty: Ty) -> Field {
    Field {
        key,
        label,
        ty,
        grants: &[],
    }
}

const fn only(field: Field, grants: &'static [&'static str]) -> Field {
    Field { grants, ..field }
}

pub struct Scheme {
    /// serde tag of the `AuthConfig` variant.
    pub kind: &'static str,
    pub label: &'static str,
    /// What the scheme does to the request, one line.
    pub hint: &'static str,
    pub fields: &'static [Field],
}

use Ty::*;

const USER: Field = f("username", "username", Text);
const PASS: Field = f("password", "password", Secret);
const CODE: &[&str] = &["authorization_code"];
const BROWSER: &[&str] = &["authorization_code", "implicit"];
const TOKEN_URL: &[&str] = &["client_credentials", "authorization_code", "password"];

pub const SCHEMES: &[Scheme] = &[
    Scheme {
        kind: "none",
        label: "no auth",
        hint: "the request goes out unauthenticated",
        fields: &[],
    },
    Scheme {
        kind: "inherit",
        label: "inherit",
        hint: "uses the auth of the nearest folder or the workspace",
        fields: &[f("from", "from", Choice(&["folder", "workspace"]))],
    },
    Scheme {
        kind: "bearer",
        label: "bearer",
        hint: "Authorization: Bearer <token>",
        fields: &[f("token", "token", Secret)],
    },
    Scheme {
        kind: "basic",
        label: "basic",
        hint: "Authorization: Basic base64(username:password)",
        fields: &[USER, PASS],
    },
    Scheme {
        kind: "api_key",
        label: "api key",
        hint: "name = value, in the header or the query",
        fields: &[
            f("key", "name", Text),
            f("value", "value", Secret),
            f("location", "send in", Choice(&["header", "query"])),
        ],
    },
    Scheme {
        kind: "digest",
        label: "digest",
        hint: "answers the server's 401 challenge (RFC 7616)",
        fields: &[USER, PASS],
    },
    Scheme {
        kind: "ntlm",
        label: "ntlm",
        hint: "NTLMv2 handshake on a dedicated connection (http only)",
        fields: &[
            USER,
            PASS,
            f("domain", "domain", Text),
            f("workstation", "workstation", Text),
        ],
    },
    Scheme {
        kind: "aws_sig_v4",
        label: "aws sigv4",
        hint: "signs the final request with AWS Signature V4",
        fields: &[
            f("access_key", "access key", Text),
            f("secret_key", "secret key", Secret),
            f("session_token", "session token", Secret),
            f("region", "region", Text),
            f("service", "service", Text),
        ],
    },
    Scheme {
        kind: "oauth1",
        label: "oauth 1.0",
        hint: "signs with OAuth 1.0 (RFC 5849)",
        fields: &[
            f("consumer_key", "consumer key", Text),
            f("consumer_secret", "consumer secret", Secret),
            f("token", "token", Text),
            f("token_secret", "token secret", Secret),
            f(
                "signature_method",
                "signature",
                Choice(&[
                    "hmac_sha1",
                    "hmac_sha256",
                    "hmac_sha512",
                    "rsa_sha1",
                    "rsa_sha256",
                    "rsa_sha512",
                    "plain_text",
                ]),
            ),
            f("params_location", "params in", Choice(&["header", "query"])),
            f("realm", "realm", Text),
        ],
    },
    Scheme {
        kind: "oauth2",
        label: "oauth 2.0",
        hint: "gets the token and sends Authorization: Bearer",
        fields: &[
            f(
                "grant_type",
                "grant",
                Choice(&[
                    "client_credentials",
                    "authorization_code",
                    "password",
                    "implicit",
                ]),
            ),
            only(f("auth_url", "auth url", Text), BROWSER),
            only(f("token_url", "token url", Text), TOKEN_URL),
            f("client_id", "client id", Text),
            only(f("client_secret", "client secret", Secret), TOKEN_URL),
            only(USER, &["password"]),
            only(PASS, &["password"]),
            f("scope", "scope", Text),
            f("audience", "audience", Text),
            only(f("redirect_uri", "redirect uri", Text), BROWSER),
            only(f("use_pkce", "pkce", Flag(true)), CODE),
            only(
                f(
                    "client_auth",
                    "credentials in",
                    Choice(&["basic_header", "request_body"]),
                ),
                TOKEN_URL,
            ),
        ],
    },
];

pub fn scheme_ix(auth: &AuthConfig) -> usize {
    let kind = object(auth)
        .get("kind")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default();
    SCHEMES.iter().position(|s| s.kind == kind).unwrap_or(0)
}

/// The serde object of `auth` (`{"kind": "bearer", "token": …}`).
pub fn object(auth: &AuthConfig) -> Map<String, Value> {
    match serde_json::to_value(auth) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// What the form shows for `field` given the stored object (flags as
/// `"true"`/`"false"`, missing keys as the serde default).
pub fn initial(field: &Field, base: &Map<String, Value>) -> String {
    match (base.get(field.key), field.ty) {
        (Some(Value::String(s)), _) => s.clone(),
        (Some(Value::Bool(b)), _) => b.to_string(),
        (_, Choice(opts)) => opts[0].to_string(),
        (_, Flag(default)) => default.to_string(),
        _ => String::new(),
    }
}

/// The scheme's form values laid over `base`: keys the form doesn't own keep
/// their stored value, so an untouched form rebuilds the stored auth exactly.
pub fn build(scheme: &Scheme, base: &Map<String, Value>, values: &[String]) -> Option<AuthConfig> {
    let mut obj = base.clone();
    obj.insert("kind".into(), scheme.kind.into());
    for (field, value) in scheme.fields.iter().zip(values) {
        let v = match field.ty {
            Flag(_) => Value::Bool(value == "true"),
            _ => Value::String(value.clone()),
        };
        obj.insert(field.key.into(), v);
    }
    serde_json::from_value(Value::Object(obj)).ok()
}

/// Whether `field` shows for the current OAuth 2 `grant` (others always do).
pub fn visible(field: &Field, grant: &str) -> bool {
    field.grants.is_empty() || field.grants.contains(&grant)
}

#[cfg(test)]
mod tests {
    use super::{build, initial, object, scheme_ix, SCHEMES};
    use serde_json::json;
    use yuki_core::AuthConfig;

    fn auth(v: serde_json::Value) -> AuthConfig {
        serde_json::from_value(v).unwrap()
    }

    fn round_trip(auth: AuthConfig) {
        let scheme = &SCHEMES[scheme_ix(&auth)];
        let base = object(&auth);
        let values: Vec<String> = scheme.fields.iter().map(|f| initial(f, &base)).collect();
        assert_eq!(build(scheme, &base, &values), Some(auth), "{}", scheme.kind);
    }

    #[test]
    fn untouched_form_rebuilds_the_stored_auth() {
        round_trip(AuthConfig::None);
        round_trip(auth(json!({"kind": "inherit", "from": "workspace"})));
        round_trip(auth(
            json!({"kind": "bearer", "token": "{{ TOKEN }}", "token_encrypted": true}),
        ));
        round_trip(auth(
            json!({"kind": "basic", "username": "u", "password": "p", "enabled": false}),
        ));
        round_trip(auth(
            json!({"kind": "api_key", "key": "X-Key", "value": "v", "location": "query"}),
        ));
        round_trip(auth(json!({"kind": "digest", "username": "u"})));
        round_trip(auth(
            json!({"kind": "ntlm", "username": "u", "domain": "d"}),
        ));
        round_trip(auth(
            json!({"kind": "aws_sig_v4", "access_key": "a", "secret_key": "s",
            "region": "us-east-1", "service": "s3"}),
        ));
        // Fields the form doesn't show (private key, nonce, state) must survive.
        round_trip(auth(
            json!({"kind": "oauth1", "consumer_key": "k", "consumer_secret": "s",
            "signature_method": "rsa_sha256", "private_key": "PEM", "nonce": "n"}),
        ));
        round_trip(auth(
            json!({"kind": "oauth2", "grant_type": "authorization_code",
            "auth_url": "a", "token_url": "t", "client_id": "c", "use_pkce": false,
            "state": "st"}),
        ));
    }

    #[test]
    fn every_scheme_builds_from_an_empty_form() {
        for (ix, scheme) in SCHEMES.iter().enumerate() {
            let base = Default::default();
            let values: Vec<String> = scheme.fields.iter().map(|f| initial(f, &base)).collect();
            let built = build(scheme, &base, &values);
            assert!(
                built.is_some(),
                "{} needs a field the form lacks",
                scheme.kind
            );
            assert_eq!(scheme_ix(&built.unwrap()), ix, "{}", scheme.kind);
        }
    }
}
