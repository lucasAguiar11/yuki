//! Format auto-detection from raw content. Every format's parser is wired into
//! `crate::parse` (Voleeo bundles go through `crate::plan` directly).

use crate::util::parse_value_opt;
use serde_json::Value;

/// The collection formats Voleeo can import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ImportFormat {
    OpenApi,
    Swagger2,
    Postman,
    Insomnia,
    Bruno,
    Yaak,
    /// Native Voleeo/Yuki bundle (`yuki_core::VoleeoBundle`).
    Voleeo,
}

/// Sniff the format from JSON/YAML content. Returns `None` when nothing matches.
pub fn detect_format(content: &str) -> Option<ImportFormat> {
    let v = parse_value_opt(content)?;

    // Voleeo bundles, Bruno OpenCollection (YAML) and Yaak (JSON) carry unique root keys.
    if v.get("voleeoBundle").is_some() {
        return Some(ImportFormat::Voleeo);
    }
    if v.get("opencollection").is_some() {
        return Some(ImportFormat::Bruno);
    }
    if v.get("yaakSchema").is_some() {
        return Some(ImportFormat::Yaak);
    }
    if v.get("swagger")
        .and_then(Value::as_str)
        .is_some_and(|s| s.starts_with("2."))
    {
        return Some(ImportFormat::Swagger2);
    }
    if v.get("openapi").is_some() {
        return Some(ImportFormat::OpenApi);
    }
    // `schema.getpostman.com` (≤2023) and `schema.postman.com` (current).
    let postman_schema = v
        .pointer("/info/schema")
        .and_then(Value::as_str)
        .is_some_and(|s| s.contains("postman.com"));
    // Environment/globals files: `{ name, values: [...] }`, scope tag optional.
    let postman_env = v.get("_postman_variable_scope").is_some()
        || (v.get("values").is_some_and(Value::is_array)
            && v.get("name").is_some_and(Value::is_string));
    if postman_schema || v.pointer("/info/_postman_id").is_some() || postman_env {
        return Some(ImportFormat::Postman);
    }
    // v5 YAML: collections, design documents (`spec.…`, carry a `collection`)
    // and standalone environments.
    let v5 = v.get("type").and_then(Value::as_str).is_some_and(|t| {
        [
            crate::insomnia_v5::V5_TYPE,
            "spec.insomnia.rest/5.0",
            "environment.insomnia.rest/5.0",
        ]
        .contains(&t)
    });
    if v.get("__export_format").is_some()
        || v.get("_type").and_then(Value::as_str) == Some("export")
        || v5
    {
        return Some(ImportFormat::Insomnia);
    }
    None
}
