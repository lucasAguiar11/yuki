use gpui_kit::component::ActiveTheme;
use gpui_kit::{App, Hsla};

/// Status keeps the strongest signal; methods use the same palette, quieter.
pub fn status_color(status: u16, cx: &App) -> Hsla {
    let t = cx.theme();
    match status {
        200..=299 => t.green,
        300..=399 => t.blue,
        400..=499 => t.yellow,
        _ => t.red,
    }
}

pub fn method_color(method: &str, cx: &App) -> Hsla {
    let t = cx.theme();
    let c = match method {
        "GET" => t.green,
        "POST" => t.yellow,
        "PUT" => t.blue,
        "PATCH" => t.magenta,
        "DELETE" => t.red,
        _ => t.cyan,
    };
    c.opacity(0.85)
}

/// Tertiary text (key hints, separators): dimmer than muted, still ≥ 4:1.
pub fn faint(cx: &App) -> Hsla {
    cx.theme().muted_foreground.opacity(0.75)
}

/// Fixed-width method label for the sidebar and tabs.
pub fn method_short(method: &str) -> &str {
    match method {
        "DELETE" => "DEL",
        "OPTIONS" => "OPT",
        "PATCH" => "PTCH",
        m => m,
    }
}

pub fn human_bytes(n: u32) -> String {
    match n {
        0..1024 => format!("{n} B"),
        1024..1_048_576 => format!("{:.1} KB", n as f64 / 1024.0),
        _ => format!("{:.1} MB", n as f64 / 1_048_576.0),
    }
}

/// serde tag of a model enum (`"json"`, `"bearer"`, …) for the meta line.
pub fn kind_name(value: impl serde::Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(v) => v["kind"].as_str().unwrap_or("?").replace('_', " "),
        Err(_) => "?".into(),
    }
}

/// History stamps are UTC (`2026-10-06T23:13:56.123Z`); show local `20:13:56`.
pub fn clock(iso: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(iso) {
        Ok(t) => t
            .with_timezone(&chrono::Local)
            .format("%H:%M:%S")
            .to_string(),
        Err(_) => iso
            .split_once('T')
            .map_or(iso, |(_, t)| t.get(..8).unwrap_or(t))
            .to_string(),
    }
}
