//! Toolbox (⌘⇧T): pasted text → derived values (base64, JWT, URL, hashes,
//! timestamps, QR) and generators (password, UUID). Pure; the panel is
//! `toolbox_panel.rs`.

use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE_NO_PAD},
    Engine as _,
};
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, SecondsFormat, Utc};
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use rand::RngExt;
use serde_json::Value;
use sha2::{Digest, Sha256, Sha512};
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Base64,
    Jwt,
    Url,
    Hash,
    Time,
    Qr,
    Password,
    Uuid,
}

impl Tool {
    pub const ALL: [Tool; 8] = [
        Tool::Base64,
        Tool::Jwt,
        Tool::Url,
        Tool::Hash,
        Tool::Time,
        Tool::Qr,
        Tool::Password,
        Tool::Uuid,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Base64 => "base64",
            Tool::Jwt => "jwt",
            Tool::Url => "url",
            Tool::Hash => "hash",
            Tool::Time => "timestamp",
            Tool::Qr => "qr code",
            Tool::Password => "password",
            Tool::Uuid => "uuid",
        }
    }

    /// Works on the shared text input; the others generate.
    pub fn takes_text(self) -> bool {
        !matches!(self, Tool::Password | Tool::Uuid)
    }
}

/// A derived value: its label and the value, or why there's none.
pub type Out = (&'static str, Result<String, &'static str>);

/// RFC 3986 unreserved characters stay as they are.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

pub fn derive(tool: Tool, text: &str) -> Vec<Out> {
    match tool {
        Tool::Base64 => vec![
            ("encoded", Ok(STANDARD.encode(text))),
            ("url-safe", Ok(URL_SAFE_NO_PAD.encode(text))),
            ("decoded", b64_decode(text).and_then(utf8)),
        ],
        Tool::Url => vec![
            (
                "encoded",
                Ok(utf8_percent_encode(text, COMPONENT).to_string()),
            ),
            ("decoded", utf8(percent_decode_str(text.trim()).collect())),
        ],
        Tool::Hash => {
            let b = text.as_bytes();
            vec![
                ("md5", Ok(format!("{:x}", md5::Md5::digest(b)))),
                ("sha1", Ok(format!("{:x}", sha1::Sha1::digest(b)))),
                ("sha256", Ok(format!("{:x}", Sha256::digest(b)))),
                ("sha512", Ok(format!("{:x}", Sha512::digest(b)))),
            ]
        }
        Tool::Jwt => jwt(text),
        Tool::Time => time(text),
        Tool::Qr | Tool::Password | Tool::Uuid => vec![],
    }
}

fn utf8(bytes: Vec<u8>) -> Result<String, &'static str> {
    String::from_utf8(bytes).map_err(|_| "not utf-8 text")
}

/// Standard or url-safe, padded or not, line breaks ignored.
fn b64_decode(text: &str) -> Result<Vec<u8>, &'static str> {
    let s: String = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            c => c,
        })
        .collect();
    STANDARD_NO_PAD
        .decode(s.trim_end_matches('='))
        .map_err(|_| "not base64")
}

/// Header and payload, plus the time claims as local dates. The signature
/// isn't checked: that needs the issuer's key.
fn jwt(text: &str) -> Vec<Out> {
    let token = text.trim();
    let token = token.strip_prefix("Bearer ").unwrap_or(token).trim();
    let mut parts = token.split('.');
    let (Some(head), Some(body)) = (parts.next(), parts.next()) else {
        return vec![("jwt", Err("not a jwt"))];
    };
    let json = |part: &str| -> Result<Value, &'static str> {
        serde_json::from_slice(&b64_decode(part)?).map_err(|_| "not json")
    };
    let pretty = |v: Value| serde_json::to_string_pretty(&v).unwrap_or_default();
    let payload = json(body);
    let mut out = vec![
        ("header", json(head).map(pretty)),
        ("payload", payload.clone().map(pretty)),
    ];
    let Ok(claims) = payload else { return out };
    let now = Utc::now().timestamp();
    for (claim, label) in [
        ("exp", "expires"),
        ("iat", "issued at"),
        ("nbf", "not before"),
    ] {
        if let Some(secs) = claims.get(claim).and_then(Value::as_i64) {
            let label = if claim == "exp" && secs < now {
                "expired"
            } else {
                label
            };
            let date = DateTime::from_timestamp(secs, 0).ok_or("not a date");
            out.push((label, date.map(local)));
        }
    }
    out
}

fn local(at: DateTime<Utc>) -> String {
    at.with_timezone(&Local)
        .format("%Y-%m-%d %H:%M:%S %:z")
        .to_string()
}

/// Empty = now; digits = unix seconds, or milliseconds from 12 digits on
/// (seconds only get there in the year 5138); else RFC 3339 or a local date.
fn time(text: &str) -> Vec<Out> {
    let t = text.trim();
    let at = if t.is_empty() {
        Some(Utc::now())
    } else if let Ok(n) = t.parse::<i64>() {
        if n.abs() >= 100_000_000_000 {
            DateTime::from_timestamp_millis(n)
        } else {
            DateTime::from_timestamp(n, 0)
        }
    } else {
        DateTime::parse_from_rfc3339(t)
            .map(|d| d.to_utc())
            .ok()
            .or_else(|| {
                let naive = ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"]
                    .iter()
                    .find_map(|f| NaiveDateTime::parse_from_str(t, f).ok())
                    .or_else(|| {
                        NaiveDate::parse_from_str(t, "%Y-%m-%d")
                            .ok()?
                            .and_hms_opt(0, 0, 0)
                    })?;
                Some(naive.and_local_timezone(Local).earliest()?.to_utc())
            })
    };
    let Some(at) = at else {
        return vec![("date", Err("unrecognized date"))];
    };
    vec![
        ("unix", Ok(at.timestamp().to_string())),
        ("unix ms", Ok(at.timestamp_millis().to_string())),
        ("utc", Ok(at.to_rfc3339_opts(SecondsFormat::Millis, true))),
        ("local", Ok(local(at))),
    ]
}

/// Uniform over the alphabet, redrawn until every class shows up (sites
/// demand one of each); `len` is raised to the number of classes.
pub fn password(len: usize, symbols: bool) -> String {
    const SETS: [&[u8]; 4] = [
        b"abcdefghijklmnopqrstuvwxyz",
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZ",
        b"0123456789",
        b"!@#$%^&*-_=+?",
    ];
    let sets = if symbols { &SETS[..] } else { &SETS[..3] };
    let all = sets.concat();
    let len = len.max(sets.len());
    let mut rng = rand::rng();
    loop {
        let p: String = (0..len)
            .map(|_| char::from(all[rng.random_range(0..all.len())]))
            .collect();
        if sets.iter().all(|s| p.bytes().any(|b| s.contains(&b))) {
            return p;
        }
    }
}

/// Namespaces for the name-based versions (RFC 9562 §6.6).
pub const NAMESPACES: [(&str, Uuid); 4] = [
    ("dns", Uuid::NAMESPACE_DNS),
    ("url", Uuid::NAMESPACE_URL),
    ("oid", Uuid::NAMESPACE_OID),
    ("x500", Uuid::NAMESPACE_X500),
];

/// One of each generated version, the usual ones first. v1/v6 get a random
/// node id (multicast bit set, as RFC 9562 asks) instead of this machine's MAC.
pub fn uuids() -> Vec<Out> {
    let mut node: [u8; 6] = rand::random();
    node[0] |= 1;
    vec![
        ("v4 · random", Ok(Uuid::new_v4().to_string())),
        ("v7 · time-ordered", Ok(Uuid::now_v7().to_string())),
        ("v1 · time + node", Ok(Uuid::now_v1(&node).to_string())),
        ("v6 · v1, sortable", Ok(Uuid::now_v6(&node).to_string())),
    ]
}

/// v3 (MD5) and v5 (SHA-1) of `name` in `NAMESPACES[ns]`: the same name always
/// gives the same UUID.
pub fn named_uuids(ns: usize, name: &str) -> Vec<Out> {
    if name.is_empty() {
        return vec![];
    }
    let ns = &NAMESPACES[ns].1;
    vec![
        (
            "v3 · md5",
            Ok(Uuid::new_v3(ns, name.as_bytes()).to_string()),
        ),
        (
            "v5 · sha-1",
            Ok(Uuid::new_v5(ns, name.as_bytes()).to_string()),
        ),
    ]
}

/// The QR's side in modules and its modules row by row (`true` = dark).
pub fn qr(text: &str) -> Result<(usize, Vec<bool>), &'static str> {
    let code = qrcode::QrCode::new(text).map_err(|_| "too long for a qr code")?;
    let dark = code
        .to_colors()
        .into_iter()
        .map(|c| c == qrcode::Color::Dark);
    Ok((code.width(), dark.collect()))
}

#[cfg(test)]
mod tests {
    use super::{derive, named_uuids, password, qr, uuids, Tool};

    fn get(tool: Tool, text: &str, label: &str) -> Result<String, &'static str> {
        derive(tool, text)
            .into_iter()
            .find(|(l, _)| *l == label)
            .map(|(_, v)| v)
            .unwrap_or(Err("missing"))
    }

    #[test]
    fn base64_decodes_any_variant() {
        assert_eq!(get(Tool::Base64, "aGk/Pz8=", "decoded"), Ok("hi???".into()));
        assert_eq!(
            get(Tool::Base64, "aGk_Pz8\n", "decoded"),
            Ok("hi???".into())
        );
        assert_eq!(get(Tool::Base64, "hi???", "url-safe"), Ok("aGk_Pz8".into()));
        assert_eq!(get(Tool::Base64, "no!", "decoded"), Err("not base64"));
    }

    #[test]
    fn url_keeps_unreserved() {
        assert_eq!(
            get(Tool::Url, "a b/ç-_.~", "encoded"),
            Ok("a%20b%2F%C3%A7-_.~".into())
        );
        assert_eq!(
            get(Tool::Url, "a%20b%2F%C3%A7", "decoded"),
            Ok("a b/ç".into())
        );
    }

    #[test]
    fn jwt_reads_claims_and_expiry() {
        // {"alg":"none"}.{"sub":"1","exp":1}
        let token = "Bearer eyJhbGciOiJub25lIn0.eyJzdWIiOiIxIiwiZXhwIjoxfQ.";
        let out = derive(Tool::Jwt, token);
        assert!(out[1].1.as_ref().unwrap().contains("\"sub\": \"1\""));
        assert!(out.iter().any(|(l, v)| *l == "expired" && v.is_ok()));
        assert_eq!(get(Tool::Jwt, "abc", "jwt"), Err("not a jwt"));
    }

    #[test]
    fn time_tells_seconds_from_millis() {
        assert_eq!(
            get(Tool::Time, "1700000000", "unix ms"),
            Ok("1700000000000".into())
        );
        assert_eq!(
            get(Tool::Time, "1700000000123", "unix"),
            Ok("1700000000".into())
        );
        assert_eq!(
            get(Tool::Time, "2023-11-14T22:13:20Z", "unix"),
            Ok("1700000000".into())
        );
        assert_eq!(
            get(Tool::Time, "yesterday", "date"),
            Err("unrecognized date")
        );
    }

    #[test]
    fn password_has_every_class() {
        for _ in 0..50 {
            let p = password(4, true);
            assert_eq!(p.len(), 4);
            assert!(p.bytes().any(|b| b.is_ascii_digit()));
            assert!(p.bytes().any(|b| !b.is_ascii_alphanumeric()));
        }
        assert!(password(30, false)
            .bytes()
            .all(|b| b.is_ascii_alphanumeric()));
    }

    #[test]
    fn qr_is_square() {
        let (w, modules) = qr("https://yuki.dev").unwrap();
        assert_eq!(modules.len(), w * w);
    }

    #[test]
    fn uuids_carry_their_version() {
        for (label, value) in uuids() {
            let v = uuid::Uuid::parse_str(&value.unwrap()).unwrap();
            assert!(label.starts_with(&format!("v{} ", v.get_version_num())));
        }
        // RFC 9562 appendix A examples, DNS namespace.
        let named = named_uuids(0, "www.example.com");
        assert_eq!(
            named[0].1,
            Ok("5df41881-3aed-3515-88a7-2f4a814cf09e".into())
        );
        assert_eq!(
            named[1].1,
            Ok("2ed6657d-e927-568b-95e1-2665a8aea6a2".into())
        );
    }
}
