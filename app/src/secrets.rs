//! Secrets at rest (auth fields, secret env variables). Encrypted workspaces
//! store `enc:v1:` ciphertext; the editors work on plaintext — opened on
//! load, sealed on save.

use std::mem::discriminant;

use yuki_core::{AuthConfig, EnvironmentVariable, HttpRequest, YukiError};
use yuki_mcp::api::ApiBackend;

/// The workspace key when the workspace is encrypted, `Ok(None)` when it isn't.
pub fn key(api: &ApiBackend, ws_id: &str) -> Result<Option<[u8; 32]>, YukiError> {
    if !api.workspaces.get(ws_id).is_ok_and(|w| w.encrypted) {
        return Ok(None);
    }
    let dir = &api.app_data_dir;
    // File first: the keychain can block waiting for a prompt.
    yuki_crypto::load_key_from_file(ws_id, dir)
        .or_else(|_| yuki_crypto::load_key(ws_id, dir))
        .map(Some)
}

/// Plaintext copy of `auth`; a secret that fails to decrypt stays as stored.
pub fn open(mut auth: AuthConfig, key: &[u8; 32]) -> AuthConfig {
    for (secret, encrypted) in auth.secret_fields_mut() {
        if encrypted && yuki_crypto::is_encrypted(secret) {
            if let Ok(plain) = yuki_crypto::decrypt(secret, key) {
                *secret = plain;
            }
        }
    }
    auth
}

/// `auth` as it goes to disk. A secret whose plaintext didn't change keeps its
/// stored ciphertext: re-encrypting draws a fresh nonce, which would rewrite
/// the file (and the user's git history) on every save.
pub fn seal(
    auth: &AuthConfig,
    stored: &AuthConfig,
    key: &[u8; 32],
) -> Result<AuthConfig, YukiError> {
    let mut stored = stored.clone();
    let old: Vec<String> = if discriminant(auth) == discriminant(&stored) {
        stored
            .secret_fields_mut()
            .into_iter()
            .map(|(s, _)| s.clone())
            .collect()
    } else {
        vec![]
    };
    let mut out = auth.clone();
    out.mark_secrets_encrypted();
    for (ix, (secret, _)) in out.secret_fields_mut().into_iter().enumerate() {
        seal_one(secret, old.get(ix).map(String::as_str), key)?;
    }
    Ok(out)
}

/// `seal` for the base auth and each variation's, a variation matched to its
/// stored self by id.
pub fn seal_request(
    req: &mut HttpRequest,
    stored: &HttpRequest,
    key: &[u8; 32],
) -> Result<(), YukiError> {
    req.auth = seal(&req.auth, &stored.auth, key)?;
    for v in &mut req.variations {
        let Some(auth) = &v.auth else { continue };
        let prev = stored
            .variations
            .iter()
            .find(|s| s.id == v.id)
            .and_then(|s| s.auth.clone())
            .unwrap_or_default();
        v.auth = Some(seal(auth, &prev, key)?);
    }
    Ok(())
}

/// Encrypt `plain` in place, reusing `old` when it already decrypts to the
/// same text: re-encrypting draws a fresh nonce, which would rewrite the file
/// (and the user's git history) on every save.
fn seal_one(plain: &mut String, old: Option<&str>, key: &[u8; 32]) -> Result<(), YukiError> {
    if yuki_crypto::is_encrypted(plain) {
        return Ok(());
    }
    *plain = match old {
        Some(c) if yuki_crypto::decrypt(c, key).ok().as_deref() == Some(plain.as_str()) => {
            c.to_string()
        }
        _ => yuki_crypto::encrypt(plain, key)?,
    };
    Ok(())
}

/// Decrypt secret variables in place; one that fails stays as stored.
pub fn open_vars(vars: &mut [EnvironmentVariable], key: &[u8; 32]) {
    for v in vars.iter_mut().filter(|v| v.encrypted) {
        if yuki_crypto::is_encrypted(&v.value) {
            if let Ok(plain) = yuki_crypto::decrypt(&v.value, key) {
                v.value = plain;
            }
        }
    }
}

/// Encrypt secret variables for disk; an unchanged one keeps the stored
/// ciphertext of the same-named variable.
pub fn seal_vars(
    vars: &mut [EnvironmentVariable],
    stored: &[EnvironmentVariable],
    key: &[u8; 32],
) -> Result<(), YukiError> {
    for v in vars.iter_mut().filter(|v| v.encrypted) {
        let old = stored.iter().find(|s| s.key == v.key && s.encrypted);
        seal_one(&mut v.value, old.map(|s| s.value.as_str()), key)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{open, open_vars, seal, seal_request, seal_vars};
    use yuki_core::{AuthConfig, EnvironmentVariable, HttpRequest, Variation};

    fn basic(user: &str, pass: &str) -> AuthConfig {
        AuthConfig::Basic {
            username: user.into(),
            password: pass.into(),
            password_encrypted: false,
            enabled: true,
        }
    }

    #[test]
    fn seal_reuses_ciphertext_for_unchanged_secrets_only() {
        let key = [7u8; 32];
        let stored = seal(&basic("u", "s3cret"), &AuthConfig::None, &key).unwrap();
        let AuthConfig::Basic {
            password: cipher,
            password_encrypted: true,
            ..
        } = &stored
        else {
            panic!("not sealed: {stored:?}")
        };
        assert!(yuki_crypto::is_encrypted(cipher));
        assert_eq!(open(stored.clone(), &key), {
            let mut b = basic("u", "s3cret");
            b.mark_secrets_encrypted();
            b
        });

        // Same plaintext (other field edited) → byte-identical ciphertext.
        let edited = seal(&basic("other", "s3cret"), &stored, &key).unwrap();
        let AuthConfig::Basic { password, .. } = &edited else {
            unreachable!()
        };
        assert_eq!(password, cipher);

        // Changed secret → fresh ciphertext of the new value.
        let changed = seal(&basic("u", "new"), &stored, &key).unwrap();
        let AuthConfig::Basic { password, .. } = open(changed, &key) else {
            unreachable!()
        };
        assert_eq!(password, "new");
    }

    #[test]
    fn seal_request_keeps_variation_ciphertext_matched_by_id() {
        let key = [7u8; 32];
        let variation = |id: &str, pass: &str| Variation {
            id: id.into(),
            name: id.into(),
            auth: Some(basic("u", pass)),
            ..Default::default()
        };
        let plain: HttpRequest = serde_json::from_value(serde_json::json!({
            "id": "r", "type": "api", "model": "http_request", "workspaceId": "w",
            "folderId": null, "method": "GET", "name": "n", "url": "u",
            "createdAt": "", "updatedAt": ""
        }))
        .unwrap();
        let mut stored = HttpRequest {
            variations: vec![variation("a", "pa"), variation("b", "pb")],
            ..plain
        };
        let empty = stored.clone();
        seal_request(
            &mut stored,
            &HttpRequest {
                variations: vec![],
                ..empty
            },
            &key,
        )
        .unwrap();
        let cipher = |r: &HttpRequest, ix: usize| match &r.variations[ix].auth {
            Some(AuthConfig::Basic { password, .. }) => password.clone(),
            other => panic!("{other:?}"),
        };
        assert!(yuki_crypto::is_encrypted(&cipher(&stored, 0)));

        // Reordered, `b` changed: `a` keeps its ciphertext, `b` gets a new one.
        let mut edited = stored.clone();
        edited.variations = vec![variation("b", "new"), variation("a", "pa")];
        seal_request(&mut edited, &stored, &key).unwrap();
        assert_eq!(cipher(&edited, 1), cipher(&stored, 0));
        assert_ne!(cipher(&edited, 0), cipher(&stored, 1));
        let Some(AuthConfig::Basic { password, .. }) =
            edited.variations[0].auth.clone().map(|a| open(a, &key))
        else {
            unreachable!()
        };
        assert_eq!(password, "new");
    }

    #[test]
    fn seal_vars_keeps_ciphertext_of_unchanged_secrets_and_skips_plain_vars() {
        let key = [7u8; 32];
        let var = |k: &str, v: &str, secret: bool| EnvironmentVariable {
            key: k.into(),
            value: v.into(),
            encrypted: secret,
            enabled: true,
        };
        let mut stored = vec![var("TOKEN", "t0k", true), var("BASE", "http://x", false)];
        seal_vars(&mut stored, &[], &key).unwrap();
        assert!(yuki_crypto::is_encrypted(&stored[0].value));
        assert_eq!(stored[1].value, "http://x");

        // Same secret, another var edited → identical ciphertext (no git diff).
        let mut edited = vec![var("TOKEN", "t0k", true), var("BASE", "http://y", false)];
        seal_vars(&mut edited, &stored, &key).unwrap();
        assert_eq!(edited[0].value, stored[0].value);

        // Changed secret → new ciphertext that opens to the new value.
        let mut changed = vec![var("TOKEN", "new", true)];
        seal_vars(&mut changed, &stored, &key).unwrap();
        assert_ne!(changed[0].value, stored[0].value);
        open_vars(&mut changed, &key);
        assert_eq!(changed[0].value, "new");
    }
}
