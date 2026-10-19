//! Allowlisted OS environment variables — the lowest-precedence resolution
//! layer. Values come from a once-per-run login-shell snapshot (GUI apps on
//! macOS don't inherit `~/.zshrc` exports); the per-workspace allowlist lives
//! in machine-local `workspace-settings.yaml` so it never git-syncs.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

/// Full OS env snapshot, captured once per app run. Refresh = restart the app.
pub fn snapshot() -> &'static HashMap<String, String> {
    static SNAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    SNAP.get_or_init(|| capture_shell_env().unwrap_or_else(|| std::env::vars().collect()))
}

/// Snapshot values for the workspace's allowlisted names. An empty allowlist
/// short-circuits so the shell capture never runs unless the user opted in.
pub(super) fn allowlisted_vars(workspace_id: &str, app_data_dir: &Path) -> HashMap<String, String> {
    let allow = allowlist(workspace_id, app_data_dir);
    if allow.is_empty() {
        return HashMap::new();
    }
    let snap = snapshot();
    allow
        .into_iter()
        .filter_map(|k| snap.get(&k).map(|v| (k, v.clone())))
        .collect()
}

const SETTINGS_FILE: &str = "workspace-settings.yaml";

/// Lenient read of one field of the workspace's entry in
/// `workspace-settings.yaml` (canonical schema:
/// `src-tauri/src/commands/workspace/settings.rs`).
fn setting<T: serde::de::DeserializeOwned>(
    workspace_id: &str,
    app_data_dir: &Path,
    key: &str,
) -> Option<T> {
    use serde_yaml::Value;
    let text = std::fs::read_to_string(app_data_dir.join(SETTINGS_FILE)).ok()?;
    let root: Value = serde_yaml::from_str(&text).ok()?;
    let entry = root
        .get("workspaces")?
        .as_sequence()?
        .iter()
        .find(|e| e.get("id").and_then(Value::as_str) == Some(workspace_id))?;
    serde_yaml::from_value(entry.get(key)?.clone()).ok()
}

pub fn allowlist(workspace_id: &str, app_data_dir: &Path) -> Vec<String> {
    setting(workspace_id, app_data_dir, "systemEnvAllowlist").unwrap_or_default()
}

/// Accept invalid (expired, self-signed) TLS certificates for this
/// workspace's sends. Machine-local on purpose: a pulled repo can't turn it on.
pub fn skip_cert_check(workspace_id: &str, app_data_dir: &Path) -> bool {
    setting(workspace_id, app_data_dir, "skipCertificateCheck").unwrap_or(false)
}

pub fn set_allowlist(
    workspace_id: &str,
    app_data_dir: &Path,
    names: &[String],
) -> Result<(), yuki_core::YukiError> {
    let names = names.iter().map(|n| n.as_str().into()).collect();
    set_setting(
        workspace_id,
        app_data_dir,
        "systemEnvAllowlist",
        serde_yaml::Value::Sequence(names),
    )
}

pub fn set_skip_cert_check(
    workspace_id: &str,
    app_data_dir: &Path,
    skip: bool,
) -> Result<(), yuki_core::YukiError> {
    set_setting(
        workspace_id,
        app_data_dir,
        "skipCertificateCheck",
        skip.into(),
    )
}

/// Sets one field of the workspace's entry. The file holds other machine-local
/// settings (Voleeo's too), so it is edited as a YAML tree: unknown keys stay.
fn set_setting(
    workspace_id: &str,
    app_data_dir: &Path,
    key: &str,
    value: serde_yaml::Value,
) -> Result<(), yuki_core::YukiError> {
    use serde_yaml::{Mapping, Value};
    let path = app_data_dir.join(SETTINGS_FILE);
    let mut root = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_yaml::from_str::<Value>(&t).ok())
        .filter(Value::is_mapping)
        .unwrap_or_else(|| Value::Mapping(Mapping::new()));
    let map = root.as_mapping_mut().expect("filtered to a mapping");
    let list = map
        .entry("workspaces".into())
        .or_insert_with(|| Value::Sequence(vec![]));
    if !list.is_sequence() {
        *list = Value::Sequence(vec![]);
    }
    let list = list.as_sequence_mut().expect("just made a sequence");
    let pos = list
        .iter()
        .position(|e| e.get("id").and_then(Value::as_str) == Some(workspace_id));
    let entry = match pos {
        Some(ix) => &mut list[ix],
        None => {
            let mut m = Mapping::new();
            m.insert("id".into(), workspace_id.into());
            list.push(Value::Mapping(m));
            list.last_mut().expect("just pushed")
        }
    };
    if let Some(m) = entry.as_mapping_mut() {
        m.insert(key.into(), value);
    }
    let text =
        serde_yaml::to_string(&root).map_err(|e| yuki_core::YukiError::Storage(e.to_string()))?;
    yuki_storage::write_atomic(path, text)
}

/// Spawn the user's login shell once and dump its env — the VS Code approach.
/// `-i` because zsh only sources `~/.zshrc` (where people export) when
/// interactive. Hard 5s timeout: a broken rc file must not stall a send.
#[cfg(unix)]
fn capture_shell_env() -> Option<HashMap<String, String>> {
    use std::io::Read;
    use std::process::{Command, Stdio};

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    let mut child = Command::new(&shell)
        .args(["-ilc", "env -0"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    match rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(buf) => {
            let _ = child.wait();
            let vars = parse_env_output(&buf);
            // Empty means the shell or `env -0` failed — treat as capture failure.
            (!vars.is_empty()).then_some(vars)
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

/// Windows GUI apps inherit user env from the registry — process env suffices.
#[cfg(windows)]
fn capture_shell_env() -> Option<HashMap<String, String>> {
    None
}

/// NUL-separated `KEY=VALUE` entries. Non-identifier keys are dropped — rc
/// files that echo to stdout fuse junk into the first chunk; this filters it.
fn parse_env_output(buf: &[u8]) -> HashMap<String, String> {
    String::from_utf8_lossy(buf)
        .split('\0')
        .filter_map(|chunk| chunk.split_once('='))
        .filter(|(k, _)| super::vars::is_identifier(k))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nul_separated_pairs() {
        let out = parse_env_output(b"HOME=/Users/me\0PATH=/bin:/usr/bin\0EMPTY=\0");
        assert_eq!(out["HOME"], "/Users/me");
        assert_eq!(out["PATH"], "/bin:/usr/bin");
        assert_eq!(out["EMPTY"], "");
    }

    #[test]
    fn drops_rc_junk_and_invalid_keys() {
        let out = parse_env_output(b"welcome banner\nHOME=/Users/me\0REAL=1\0no-dash=x\0");
        assert!(!out.contains_key("HOME"), "junk-fused chunk dropped");
        assert_eq!(out["REAL"], "1");
        assert!(!out.contains_key("no-dash"));
    }

    #[test]
    fn reads_allowlist_for_matching_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("workspace-settings.yaml"),
            "workspaces:\n- id: ws1\n  activeEnvId: dev\n  systemEnvAllowlist:\n  - HOME\n  - API_KEY\n- id: ws2\n",
        )
        .unwrap();
        assert_eq!(allowlist("ws1", dir.path()), vec!["HOME", "API_KEY"]);
        assert!(allowlist("ws2", dir.path()).is_empty());
        assert!(allowlist("missing", dir.path()).is_empty());
    }

    #[test]
    fn set_allowlist_keeps_other_keys_and_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("workspace-settings.yaml"),
            "theme: dark\nworkspaces:\n- id: ws1\n  activeEnvId: dev\n  systemEnvAllowlist:\n  - HOME\n- id: ws2\n",
        )
        .unwrap();
        set_allowlist("ws1", dir.path(), &["AWS_PROFILE".into()]).unwrap();
        set_allowlist("ws3", dir.path(), &["TOKEN".into()]).unwrap();
        assert_eq!(allowlist("ws1", dir.path()), vec!["AWS_PROFILE"]);
        assert_eq!(allowlist("ws3", dir.path()), vec!["TOKEN"]);
        let text = std::fs::read_to_string(dir.path().join("workspace-settings.yaml")).unwrap();
        assert!(text.contains("theme: dark") && text.contains("activeEnvId: dev"));
        assert!(text.contains("id: ws2"));
    }

    #[test]
    fn missing_settings_file_means_empty_allowlist() {
        let dir = tempfile::tempdir().unwrap();
        assert!(allowlist("ws1", dir.path()).is_empty());
    }
}
