//! Settings → general: send settings for every workspace. Machine-local,
//! under `global:` in `workspace-settings.yaml` (never in a workspace's git).
//! A workspace's own value wins (user-agent, a DNS host) or adds to these
//! (shell env names, accepting invalid certificates).

use std::path::Path;

use serde::{Deserialize, Serialize};
use yuki_core::{DnsOverride, YukiError};

const KEY: &str = "global";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSettings {
    /// Sent when no request/folder/workspace header sets one; empty = `Yuki`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user_agent: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip_certificate_check: bool,
    /// After the workspace's own: the first match for a host wins.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dns_overrides: Vec<DnsOverride>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub system_env_allowlist: Vec<String>,
}

impl GlobalSettings {
    /// The `User-Agent` a send falls back to.
    pub fn user_agent_or_default(&self) -> &str {
        match self.user_agent.trim() {
            "" => yuki_core::DEFAULT_USER_AGENT,
            ua => ua,
        }
    }
}

/// Lenient: a missing or unreadable file means no general settings.
pub fn settings(app_data_dir: &Path) -> GlobalSettings {
    std::fs::read_to_string(app_data_dir.join(super::system_env::SETTINGS_FILE))
        .ok()
        .and_then(|t| serde_yaml::from_str::<serde_yaml::Value>(&t).ok())
        .and_then(|root| serde_yaml::from_value(root.get(KEY)?.clone()).ok())
        .unwrap_or_default()
}

pub fn set_settings(app_data_dir: &Path, global: &GlobalSettings) -> Result<(), YukiError> {
    let value = serde_yaml::to_value(global).map_err(|e| YukiError::Storage(e.to_string()))?;
    super::system_env::edit_settings(app_data_dir, |map| {
        map.insert(KEY.into(), value);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_beside_workspace_entries() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("workspace-settings.yaml");
        std::fs::write(
            &file,
            "theme: dark\nworkspaces:\n- id: ws1\n  skipCertificateCheck: true\n",
        )
        .unwrap();
        assert_eq!(settings(dir.path()), GlobalSettings::default());
        assert_eq!(settings(dir.path()).user_agent_or_default(), "Yuki");
        let g = GlobalSettings {
            user_agent: "Mine/1".into(),
            system_env_allowlist: vec!["TOKEN".into()],
            ..Default::default()
        };
        set_settings(dir.path(), &g).unwrap();
        assert_eq!(settings(dir.path()), g);
        // The workspace entries and unknown keys survive.
        assert_eq!(super::super::skip_cert_check("ws1", dir.path()), Some(true));
        // "As general" drops the workspace's key instead of writing false.
        super::super::set_skip_cert_check("ws1", dir.path(), None).unwrap();
        assert_eq!(super::super::skip_cert_check("ws1", dir.path()), None);
        assert_eq!(settings(dir.path()), g);
        assert!(std::fs::read_to_string(&file)
            .unwrap()
            .contains("theme: dark"));
    }

    /// Settings → general vs one workspace, setting by setting.
    #[test]
    fn workspace_and_general_combine() {
        use super::super::system_env::effective_allowlist;
        use super::super::{effective_skip_cert_check as skip, set_skip_cert_check};
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let general = |accept: bool, env: &[&str]| GlobalSettings {
            skip_certificate_check: accept,
            system_env_allowlist: env.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };

        // Certificates: "as general" follows it; the workspace's own choice wins either way.
        set_settings(d, &general(true, &[])).unwrap();
        assert!(skip("ws1", d));
        set_skip_cert_check("ws1", d, Some(false)).unwrap();
        assert!(!skip("ws1", d), "verify beats a general accept");
        set_settings(d, &general(false, &[])).unwrap();
        set_skip_cert_check("ws1", d, Some(true)).unwrap();
        assert!(skip("ws1", d), "accept beats a general verify");
        set_skip_cert_check("ws1", d, None).unwrap();
        assert!(!skip("ws1", d));

        // Shell env: the lists add up, no duplicates, the workspace's first.
        set_settings(d, &general(false, &["TOKEN", "HOME"])).unwrap();
        super::super::set_system_env_allowlist("ws1", d, &["HOME".into(), "AWS".into()]).unwrap();
        assert_eq!(effective_allowlist("ws1", d), ["HOME", "AWS", "TOKEN"]);
        assert_eq!(effective_allowlist("ws2", d), ["TOKEN", "HOME"]);
    }
}
