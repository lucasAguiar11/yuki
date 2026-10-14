//! Warning bookkeeping and environment variables shared by the v4 and v5 parsers.

use super::s;
use crate::ir::ImportedVariable;
use serde_json::Value;
use std::collections::BTreeSet;

/// Things the import drops, reported as one counted warning each.
#[derive(Default)]
pub(crate) struct Notes {
    /// WebSocket/gRPC/Socket.IO items.
    pub skipped: usize,
    scripts: usize,
    /// Env keys whose value is an object/array (kept as JSON text).
    nested: BTreeSet<String>,
}

impl Notes {
    pub fn script(&mut self, item: &Value) {
        let has = [
            "/preRequestScript",
            "/afterResponseScript",
            "/scripts/preRequest",
            "/scripts/afterResponse",
        ]
        .iter()
        .any(|p| {
            item.pointer(p)
                .and_then(Value::as_str)
                .is_some_and(|s| !s.trim().is_empty())
        });
        self.scripts += usize::from(has);
    }

    /// An environment's variables: key-value envs carry `kvPairData` (disabled
    /// pairs kept as disabled); JSON ones use the `data` object.
    pub fn env_of(&mut self, e: &Value) -> Vec<ImportedVariable> {
        let Some(kv) = e
            .get("kvPairData")
            .and_then(Value::as_array)
            .filter(|a| !a.is_empty())
        else {
            return self.env(e.get("data"));
        };
        kv.iter()
            .filter_map(|p| {
                let key = p
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|k| !k.is_empty())?;
                Some(ImportedVariable {
                    key: key.to_string(),
                    value: s(p, "value"),
                    enabled: p.get("enabled").and_then(Value::as_bool) != Some(false),
                })
            })
            .collect()
    }

    /// `env_data` that also records nested values. Yuki variable names can't
    /// contain `.` (`yuki_mcp::resolve::is_identifier`), so `{{ obj.key }}`
    /// can't be kept working by flattening.
    pub fn env(&mut self, v: Option<&Value>) -> Vec<ImportedVariable> {
        if let Some(o) = v.and_then(Value::as_object) {
            let nested = o.iter().filter(|(_, v)| v.is_object() || v.is_array());
            self.nested.extend(nested.map(|(k, _)| k.clone()));
        }
        env_data(v)
    }

    pub fn warn(self, warnings: &mut Vec<String>) {
        if self.skipped > 0 {
            warnings.push(format!("{} WebSocket/gRPC item(s) skipped", self.skipped));
        }
        if self.scripts > 0 {
            warnings.push(format!(
                "{} item(s) with pre-request/after-response scripts — scripts aren't supported and were skipped",
                self.scripts
            ));
        }
        if !self.nested.is_empty() {
            let keys: Vec<&str> = self.nested.iter().map(String::as_str).collect();
            warnings.push(format!(
                "Nested environment values were imported as JSON text, so `{{{{ key.field }}}}` \
                 references to them won't resolve: {}",
                keys.join(", ")
            ));
        }
    }
}

fn env_data(v: Option<&Value>) -> Vec<ImportedVariable> {
    v.and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .map(|(k, val)| ImportedVariable {
                    key: k.clone(),
                    value: match val {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    },
                    enabled: true,
                })
                .collect()
        })
        .unwrap_or_default()
}
