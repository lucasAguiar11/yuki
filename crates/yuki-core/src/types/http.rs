//! HTTP request, folder, and response/timeline/SSE types.

use super::common::{EnvironmentVariable, RequestParameter};
use crate::auth::{is_auth_none, AuthConfig};
use crate::cookies::StoredCookie;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Type, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum BodyKind {
    #[default]
    None,
    Json,
    Xml,
    Text,
    Html,
    FormUrlEncoded,
    Multipart,
    Binary,
    Graphql,
}

/// One field of a form-urlencoded or multipart body. `is_file` (multipart only)
/// flags a file attachment whose `value` is an absolute path read at send time.
#[derive(Type, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BodyField {
    pub id: String,
    pub name: String,
    pub value: String,
    pub enabled: bool,
    #[serde(default)]
    pub is_file: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
}

/// `text` carries raw bodies (json/xml/text/html). `fields` carries
/// form-urlencoded / multipart entries. `file_path` carries the binary upload.
/// New fields are `Option` so existing `{ kind, text }` payloads stay valid.
#[derive(Type, Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RequestBody {
    pub kind: BodyKind,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<BodyField>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// GraphQL variables (JSON object string). Only meaningful for `Graphql`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graphql_variables: Option<String>,
}

impl RequestBody {
    pub fn graphql_payload(&self) -> String {
        let variables: serde_json::Value = self
            .graphql_variables
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(serde_json::Value::Null);
        serde_json::json!({ "query": self.text, "variables": variables }).to_string()
    }
}

#[derive(Type, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HttpRequest {
    pub id: String,
    #[serde(rename = "type")]
    pub request_type: String,
    pub model: String,
    pub workspace_id: String,
    pub folder_id: Option<String>,
    pub method: String,
    pub name: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<RequestParameter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<RequestParameter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<RequestBody>,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub order: f64,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variations: Vec<Variation>,
    /// Id of the variation sent by default; `None` = the base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_variation: Option<String>,
    /// Base-only (variations don't override it).
    #[serde(default, skip_serializing_if = "RequestSettings::is_default")]
    pub settings: RequestSettings,
}

/// How a request is sent. The default is what every send did before these
/// existed: no timeout, follow up to `MAX_REDIRECTS` redirects.
#[derive(Type, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestSettings {
    /// Whole send, connect to last body byte; `None` = no limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u32>,
    /// `false`: a 3xx is the response.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub follow_redirects: bool,
    /// `None` = `MAX_REDIRECTS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_redirects: Option<u32>,
}

impl RequestSettings {
    /// The engine's own redirect cap.
    pub const MAX_REDIRECTS: u32 = 10;

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Redirects to follow; 0 = none.
    pub fn redirect_limit(&self) -> u32 {
        if self.follow_redirects {
            self.max_redirects.unwrap_or(Self::MAX_REDIRECTS)
        } else {
            0
        }
    }
}

impl Default for RequestSettings {
    fn default() -> Self {
        Self {
            timeout_ms: None,
            follow_redirects: true,
            max_redirects: None,
        }
    }
}

fn yes() -> bool {
    true
}

fn is_yes(b: &bool) -> bool {
    *b
}

/// A named alternative of a request: an independent copy of its sendable
/// fields. Yuki writes all five; a `None` (hand-written, or older Yuki) falls
/// back to the base's. `Some(vec![])` = none at all.
#[derive(Type, Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Variation {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Vec<RequestParameter>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Vec<RequestParameter>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<RequestBody>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthConfig>,
}

impl Variation {
    /// A variation holding every sendable field of `req`.
    pub fn copy_of(req: &HttpRequest, id: String, name: String) -> Self {
        Variation {
            id,
            name,
            url: Some(req.url.clone()),
            parameters: Some(req.parameters.clone()),
            headers: Some(req.headers.clone()),
            body: Some(req.body.clone().unwrap_or_default()),
            auth: Some(req.auth.clone()),
        }
    }
}

impl HttpRequest {
    /// The request as sent: `pick` (variation id or name), else the active
    /// variation, laid over the base. The result carries no variations, so
    /// history and snapshots never hold the inactive ones (or their secrets).
    /// An unknown `pick` is an error; a dangling `active_variation` is the base.
    pub fn effective(&self, pick: Option<&str>) -> Result<HttpRequest, crate::YukiError> {
        let Some(p) = pick else {
            return Ok(self.shown());
        };
        let v = self
            .variations
            .iter()
            .find(|v| v.id == p)
            .or_else(|| self.variations.iter().find(|v| v.name == p))
            .ok_or_else(|| crate::YukiError::NotFound(format!("variation {p}")))?;
        Ok(self.overlay(Some(v)))
    }

    /// `effective(None)`: the active variation (if any) over the base.
    pub fn shown(&self) -> HttpRequest {
        self.overlay(self.active())
    }

    /// The variation `active_variation` names, if it exists.
    pub fn active(&self) -> Option<&Variation> {
        let id = self.active_variation.as_deref()?;
        self.variations.iter().find(|v| v.id == id)
    }

    pub fn active_mut(&mut self) -> Option<&mut Variation> {
        let id = self.active_variation.clone()?;
        self.variations.iter_mut().find(|v| v.id == id)
    }

    fn overlay(&self, v: Option<&Variation>) -> HttpRequest {
        let mut req = HttpRequest {
            variations: vec![],
            active_variation: None,
            ..self.clone()
        };
        let Some(v) = v.cloned() else { return req };
        if let Some(url) = v.url {
            req.url = url;
        }
        if let Some(p) = v.parameters {
            req.parameters = p;
        }
        if let Some(h) = v.headers {
            req.headers = h;
        }
        if let Some(b) = v.body {
            req.body = Some(b);
        }
        if let Some(a) = v.auth {
            req.auth = a;
        }
        req
    }

    /// The base auth and every variation's override, for code that seals,
    /// opens or masks secrets.
    pub fn auths_mut(&mut self) -> impl Iterator<Item = &mut AuthConfig> {
        std::iter::once(&mut self.auth)
            .chain(self.variations.iter_mut().filter_map(|v| v.auth.as_mut()))
    }

    /// Fill every field a variation doesn't hold with the base's, so it stops
    /// following later base edits.
    pub fn detach_variations(&mut self) {
        let base = Variation::copy_of(self, String::new(), String::new());
        for v in &mut self.variations {
            v.url = v.url.take().or_else(|| base.url.clone());
            v.parameters = v.parameters.take().or_else(|| base.parameters.clone());
            v.headers = v.headers.take().or_else(|| base.headers.clone());
            v.body = v.body.take().or_else(|| base.body.clone());
            v.auth = v.auth.take().or_else(|| base.auth.clone());
        }
    }
}

#[derive(Type, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApiFolder {
    pub id: String,
    #[serde(rename = "type")]
    pub folder_type: String,
    pub model: String,
    pub workspace_id: String,
    pub folder_id: Option<String>,
    pub name: String,
    /// Applied to requests in this folder + subfolders; nearest scope wins.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<RequestParameter>,
    /// Folder-level auth. Requests can opt in via `AuthConfig::Inherit`.
    #[serde(default, skip_serializing_if = "is_auth_none")]
    pub auth: AuthConfig,
    /// Variables for requests in this folder + subfolders (`{{ KEY }}`). Nearest
    /// folder wins over ancestors, folders win over envs. Ciphertext at rest.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<EnvironmentVariable>,
    /// Accent color (hex `#rrggbb`) for the folder icon; `None` = theme default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default)]
    pub order: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Type, Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponseHeader {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub at_ms: f64,
}

/// One Timeline-tab row; `at_ms` is elapsed from request start. `kind` is a
/// string (not an enum) so the executor can add types without breaking codegen.
/// Today: `config` | `info` | `send` | `recv` | `dns` | `chunk` | `done`.
#[derive(Type, Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEvent {
    pub at_ms: f64,
    pub kind: String,
    pub text: String,
}

/// One frame parsed from a `text/event-stream` response and pushed to the UI
/// live. `seq` is the 0-based arrival order within a send (the React key);
/// `data` joins multiple `data:` lines with `\n`. Omitted fields were absent in
/// the frame.
#[derive(Type, Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SseFrame {
    pub seq: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    pub data: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_id: Option<String>,
    /// SSE reconnect hint (ms). `u32` keeps it JS-number / specta safe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<u32>,
    pub at_ms: f64,
}

/// Phase timings (ms). DNS/TCP/TLS aren't available from the client → 0.0.
/// `first_byte_ms` = start→headers; `download_ms` = headers→body read.
#[derive(Type, Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct HttpTiming {
    pub dns_ms: f64,
    pub connect_ms: f64,
    pub tls_ms: f64,
    pub first_byte_ms: f64,
    pub download_ms: f64,
    pub total_ms: f64,
}

/// True when every phase is 0 — snapshots zero out timing (it's meaningless for
/// a point-in-time capture) so it's skipped from their YAML entirely.
fn is_zero_timing(t: &HttpTiming) -> bool {
    t.dns_ms == 0.0
        && t.connect_ms == 0.0
        && t.tls_ms == 0.0
        && t.first_byte_ms == 0.0
        && t.download_ms == 0.0
        && t.total_ms == 0.0
}

/// Warning surfaced when following redirects silently dropped part of the
/// request, so the result isn't mistaken for a server-side problem.
#[derive(Type, Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RedirectInfo {
    pub hop_count: u32,
    /// Body dropped when a 301/302/303 downgraded a non-GET request.
    pub body_dropped: bool,
    /// Sensitive headers stripped on a cross-origin redirect.
    pub dropped_headers: Vec<String>,
}

/// Result of executing a saved `HttpRequest` in-app (not the raw HTTP `Response` type).
#[derive(Type, Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HttpResponse {
    pub request_id: String,
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<HttpResponseHeader>,
    /// UTF-8 body or lossy; non-text bodies are base64-encoded. Empty when
    /// `body_windowed` — the body then lives in a side file fetched in windows.
    pub body: String,
    /// Byte length of the response body (`u32` for JS number / specta interop; max ~4GiB).
    pub body_size: u32,
    pub body_is_text: bool,
    /// Large text bodies are stored out-of-line and read by line window via
    /// `response_body_window`; `body` is empty and the frontend virtualizes.
    #[serde(default)]
    pub body_windowed: bool,
    /// Line count of the stored (formatted) body — sizes the virtual viewport.
    #[serde(default)]
    pub body_line_count: u32,
    /// History id used to fetch windows/search; assigned on store, else empty.
    #[serde(default)]
    pub response_id: String,
    #[serde(default, skip_serializing_if = "is_zero_timing")]
    pub timing: HttpTiming,
    /// Timeline event log; when empty the frontend reconstructs milestones
    /// from `headers` + `timing`. Skipped from YAML when empty (e.g. snapshots,
    /// which never show a timeline) — non-empty live logs still serialize.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<TimelineEvent>,
    /// Present when redirects dropped the body or sensitive headers.
    #[serde(default)]
    pub redirect_warning: Option<RedirectInfo>,
    /// Cookies from `Set-Cookie` (incl. redirect hops). Plaintext; the command
    /// layer encrypts on write when the workspace is encrypted.
    #[serde(default)]
    pub captured_cookies: Vec<StoredCookie>,
    /// Cookies the executor sent (active jar, matched at send time). Aggregated
    /// across hops, deduped by id.
    #[serde(default)]
    pub attached_cookies: Vec<StoredCookie>,
    /// Parsed frames for a `text/event-stream` response. Empty for normal
    /// responses; the body stays empty when this is populated. Persisted with
    /// the response so each run keeps its stream in history.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sse_frames: Vec<SseFrame>,
    /// A load run's log, on the one answer it keeps: a header, one entry per
    /// send (`#3 200 OK · 18 ms`, with its request and response), the summary.
    /// Empty for a plain send.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub load_log: Vec<LoadLogEntry>,
}

/// One line of a load run's log. A send keeps what went over the wire: its
/// timeline (`>` request line and headers, `<` status and headers) and the
/// start of the response body; header/summary lines have neither.
#[derive(Type, Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoadLogEntry {
    pub at_ms: f64,
    /// `info`, `done` (answered < 400) or `error`.
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<TimelineEvent>,
    /// Response body, cut to a few KB; `None` = not kept (or no answer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// Immutable, point-in-time snapshot of a fully-resolved request and the
/// response it produced — a git-synced "source of truth", distinct from
/// the machine-local response history ring buffer. Never edited after
/// creation; only `name` may be changed via a dedicated rename call.
///
/// `request` is the *resolved* `HttpRequest` captured at send time (literal
/// URL/headers/body, no `{{ }}` templates) — see `resolve::apply_to_request`.
/// When `encrypted` is true, `request.auth`'s secret fields and `response`'s
/// body/header values are AES-256-GCM ciphertext (`enc:v1:...`) under the
/// workspace key; when false, secret-sourced values are replaced with a
/// redaction placeholder rather than left as plaintext.
#[derive(Type, Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: String,
    pub workspace_id: String,
    pub request_id: String,
    pub name: String,
    pub created_at: String,
    pub encrypted: bool,
    /// Pinned snapshots sort to the top of their request's list. Cosmetic,
    /// mutable (like `name`) — the rest of the snapshot stays immutable.
    #[serde(default, skip_serializing_if = "is_snapshot_unpinned")]
    pub pinned: bool,
    pub request: HttpRequest,
    pub response: HttpResponse,
}

fn is_snapshot_unpinned(pinned: &bool) -> bool {
    !*pinned
}

/// Lightweight `Snapshot` listing entry — the sidebar tree needs every
/// request's snapshots at workspace load, and full snapshots carry whole bodies.
#[derive(Type, Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSummary {
    pub id: String,
    pub request_id: String,
    pub name: String,
    pub created_at: String,
    pub encrypted: bool,
    pub pinned: bool,
    pub method: String,
    pub status: u16,
}

/// Outcome of re-executing a `Snapshot`. The fresh response is stored
/// separately as the single machine-local "latest" entry (overwritten on
/// each replay, never git-synced) — this struct is just the IPC return.
#[derive(Type, Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotReplayResult {
    pub response: HttpResponse,
    pub status_matches: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphql_payload_wraps_query_and_variables() {
        let with_vars = RequestBody {
            kind: BodyKind::Graphql,
            text: "query Q($c: ID!) { country(code: $c) { name } }".into(),
            graphql_variables: Some(r#"{"c":"UA"}"#.into()),
            ..Default::default()
        };
        let v: serde_json::Value = serde_json::from_str(&with_vars.graphql_payload()).unwrap();
        assert_eq!(
            v["query"],
            "query Q($c: ID!) { country(code: $c) { name } }"
        );
        assert_eq!(v["variables"]["c"], "UA");

        let no_vars = RequestBody {
            kind: BodyKind::Graphql,
            text: "{ me }".into(),
            ..Default::default()
        };
        let v: serde_json::Value = serde_json::from_str(&no_vars.graphql_payload()).unwrap();
        assert!(v["variables"].is_null());
    }
}

#[cfg(test)]
mod variation_tests {
    use super::*;

    fn param(name: &str) -> RequestParameter {
        RequestParameter {
            id: name.into(),
            name: name.into(),
            value: "v".into(),
            enabled: true,
        }
    }

    fn req() -> HttpRequest {
        serde_json::from_value(serde_json::json!({
            "id": "r", "type": "http_request", "model": "http_request", "workspaceId": "w",
            "folderId": null, "method": "GET", "name": "n", "url": "https://base",
            "headers": [{"id": "h", "name": "h", "value": "v", "enabled": true}],
            "auth": {"kind": "bearer", "token": "base", "tokenEncrypted": false, "enabled": true},
            "createdAt": "", "updatedAt": ""
        }))
        .unwrap()
    }

    fn bearer(t: &str) -> AuthConfig {
        AuthConfig::Bearer {
            token: t.into(),
            token_encrypted: false,
            enabled: true,
        }
    }

    #[test]
    fn effective_overlays_whole_fields_and_drops_variations() {
        let mut r = req();
        r.variations = vec![
            Variation {
                id: "a".into(),
                name: "admin".into(),
                auth: Some(bearer("admin")),
                headers: Some(vec![]),
                ..Default::default()
            },
            Variation {
                id: "u".into(),
                name: "user".into(),
                url: Some("https://user".into()),
                ..Default::default()
            },
        ];
        r.active_variation = Some("u".into());

        // Active one by default; untouched fields stay the base's.
        let e = r.effective(None).unwrap();
        assert_eq!(e.url, "https://user");
        assert_eq!(e.auth, bearer("base"));
        assert_eq!(e.headers, vec![param("h")]);
        assert!(e.variations.is_empty() && e.active_variation.is_none());

        // An explicit pick (by name or id) beats the active one; `Some(vec![])` clears.
        for pick in ["admin", "a"] {
            let e = r.effective(Some(pick)).unwrap();
            assert_eq!(
                (e.url.as_str(), &e.auth),
                ("https://base", &bearer("admin"))
            );
            assert!(e.headers.is_empty());
        }

        assert!(r.effective(Some("nope")).is_err());
        r.active_variation = Some("gone".into());
        assert_eq!(r.effective(None).unwrap().url, "https://base");
    }

    #[test]
    fn empty_override_survives_round_trip_and_none_is_omitted() {
        let mut r = req();
        let plain = serde_json::to_value(&r).unwrap();
        assert!(plain.get("variations").is_none() && plain.get("activeVariation").is_none());

        r.variations = vec![Variation {
            id: "a".into(),
            name: "admin".into(),
            headers: Some(vec![]),
            ..Default::default()
        }];
        let back: HttpRequest = serde_json::from_value(serde_json::to_value(&r).unwrap()).unwrap();
        assert_eq!(back.variations[0].headers, Some(vec![]));
        assert_eq!(back.variations[0].url, None);
    }
}
