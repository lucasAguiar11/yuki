//! Auth secrets at rest. Encrypted workspaces store `enc:v1:` ciphertext; the
//! editor works on plaintext — opened on load, sealed on save.

use std::mem::discriminant;

use yuki_core::{AuthConfig, YukiError};
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
        if yuki_crypto::is_encrypted(secret) {
            continue;
        }
        match old.get(ix) {
            Some(c) if yuki_crypto::decrypt(c, key).ok().as_deref() == Some(secret.as_str()) => {
                *secret = c.clone()
            }
            _ => *secret = yuki_crypto::encrypt(secret, key)?,
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{open, seal};
    use yuki_core::AuthConfig;

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
}
