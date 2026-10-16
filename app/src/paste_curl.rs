//! A `curl …` command pasted into the URL bar becomes the open request: the
//! reverse of `curl.rs`. Flags without a request field (`-L`, `-k`,
//! `--compressed`, …) are skipped; a flag that takes a value we don't use
//! must be listed in `SKIP_VALUE` or its value would read as the URL.

use yuki_core::{
    new_id, AuthConfig, BodyField, BodyKind, HttpRequest, RequestBody, RequestParameter,
};

use crate::query;

const SKIP_VALUE: &[&str] = &[
    "-A",
    "--user-agent",
    "-b",
    "--cookie",
    "-e",
    "--referer",
    "-o",
    "--output",
    "-m",
    "--max-time",
    "--connect-timeout",
    "-x",
    "--proxy",
    "--cacert",
    "--cert",
    "--key",
    "-w",
    "--write-out",
    "-c",
    "--cookie-jar",
    "--retry",
    "-r",
    "--range",
];

/// Overwrites `req`'s method, URL, params, headers, body and auth with `cmd`'s.
/// `false` (and `req` untouched) when it isn't a cURL with a URL.
pub fn apply(cmd: &str, req: &mut HttpRequest) -> bool {
    // `\`-newline continuations (bash) and `^`-newline (cmd.exe DevTools).
    let cmd = cmd
        .replace("\\\r\n", " ")
        .replace("\\\n", " ")
        .replace("^\n", " ");
    let Some(args) = shlex::split(&cmd) else {
        return false;
    };
    let mut args = args.into_iter();
    if args.next().as_deref() != Some("curl") {
        return false;
    }
    let (mut method, mut url, mut get) = (None, None, false);
    let (mut headers, mut data, mut urlenc, mut form, mut user) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), None);
    while let Some(a) = args.next() {
        let mut val = || args.next().unwrap_or_default();
        match a.as_str() {
            "-X" | "--request" => method = Some(val()),
            "--url" => url = Some(val()),
            "-H" | "--header" => headers.push(val()),
            "-d" | "--data" | "--data-raw" | "--data-ascii" | "--data-binary" => data.push(val()),
            "--data-urlencode" => urlenc.push(val()),
            "-F" | "--form" => form.push((val(), true)),
            "--form-string" => form.push((val(), false)),
            "-u" | "--user" => user = Some(val()),
            "-G" | "--get" => get = true,
            s if SKIP_VALUE.contains(&s) => _ = val(),
            s if s.starts_with('-') => {}
            _ => url = url.or(Some(a)),
        }
    }
    let Some(url) = url else { return false };

    let headers: Vec<_> = headers
        .iter()
        .filter_map(|h| h.split_once(':'))
        .map(|(n, v)| param(n.trim(), v.trim()))
        .collect();
    let ct = headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("content-type"))
        .map(|h| h.value.to_ascii_lowercase())
        .unwrap_or_default();
    let mut pairs = query::pairs(&url);
    let has_body = !(data.is_empty() && urlenc.is_empty() && form.is_empty());

    let body = if get {
        pairs.extend(data.iter().chain(&urlenc).flat_map(|d| split_pairs(d)));
        None
    } else if !form.is_empty() {
        let fields = form.iter().filter_map(|(f, files)| {
            let (name, value) = f.split_once('=')?;
            let file = files.then(|| value.strip_prefix('@')).flatten();
            Some(BodyField {
                id: new_id(),
                name: name.into(),
                value: file.unwrap_or(value).split(";type=").next()?.into(),
                enabled: true,
                is_file: file.is_some(),
                content_type: file
                    .and_then(|f| f.split_once(";type="))
                    .map(|(_, t)| t.into()),
            })
        });
        Some(fields_body(BodyKind::Multipart, fields.collect()))
    } else if !data.is_empty() {
        let text = data.join("&");
        let json = ct.contains("json")
            || (ct.is_empty()
                && serde_json::from_str::<serde_json::Value>(&text)
                    .is_ok_and(|v| !v.is_string() && !v.is_number()));
        match () {
            _ if json => Some(text_body(BodyKind::Json, text)),
            _ if ct.contains("xml") => Some(text_body(BodyKind::Xml, text)),
            _ if ct.is_empty() || ct.contains("x-www-form-urlencoded") => {
                let fields = split_pairs(&text)
                    .into_iter()
                    .chain(urlenc.iter().flat_map(|d| split_pairs(d)));
                Some(fields_body(
                    BodyKind::FormUrlEncoded,
                    fields.map(field).collect(),
                ))
            }
            _ => Some(text_body(BodyKind::Text, text)),
        }
    } else if !urlenc.is_empty() {
        let fields = urlenc.iter().flat_map(|d| split_pairs(d)).map(field);
        Some(fields_body(BodyKind::FormUrlEncoded, fields.collect()))
    } else {
        None
    };

    req.method = method
        .map(|m| m.to_ascii_uppercase())
        .unwrap_or_else(|| if has_body && !get { "POST" } else { "GET" }.into());
    req.url = query::without_query(&url);
    let path = yuki_mcp::resolve::path_params(&req.url);
    req.parameters = path
        .into_iter()
        .map(|p| param(&p, ""))
        .chain(query::as_params(pairs))
        .collect();
    req.headers = headers;
    req.body = body;
    req.auth = match user
        .as_deref()
        .map(|u| u.split_once(':').unwrap_or((u, "")))
    {
        Some((username, password)) => AuthConfig::Basic {
            username: username.into(),
            password: password.into(),
            password_encrypted: false,
            enabled: true,
        },
        None => AuthConfig::default(),
    };
    true
}

fn split_pairs(s: &str) -> Vec<(String, String)> {
    query::pairs(&format!("?{s}"))
}

fn param(name: &str, value: &str) -> RequestParameter {
    RequestParameter {
        id: new_id(),
        name: name.into(),
        value: value.into(),
        enabled: true,
    }
}

fn field((name, value): (String, String)) -> BodyField {
    BodyField {
        id: new_id(),
        name,
        value,
        enabled: true,
        is_file: false,
        content_type: None,
    }
}

fn text_body(kind: BodyKind, text: String) -> RequestBody {
    RequestBody {
        kind,
        text,
        ..Default::default()
    }
}

fn fields_body(kind: BodyKind, fields: Vec<BodyField>) -> RequestBody {
    RequestBody {
        kind,
        fields: Some(fields),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::apply;
    use yuki_core::{AuthConfig, BodyKind, HttpRequest};

    fn blank() -> HttpRequest {
        serde_json::from_value(serde_json::json!({
            "id": "r", "type": "http_request", "model": "http_request", "workspaceId": "w",
            "folderId": null, "method": "DELETE", "name": "r", "url": "https://old",
            "createdAt": "", "updatedAt": ""
        }))
        .unwrap()
    }

    #[test]
    fn devtools_post_json_with_query() {
        let cmd = "curl 'https://api.x.dev/v1/users/:id?page=2&q=a%20b' \\\n  \
                   -H 'accept: */*' \\\n  -H 'content-type: application/json' \\\n  \
                   --data-raw '{\"name\":\"it'\\''s\"}' \\\n  --compressed";
        // The single-line URL input strips the breaks before we see the text.
        for cmd in [cmd.to_string(), cmd.replace('\n', "")] {
            let mut r = blank();
            assert!(apply(&cmd, &mut r));
            assert_eq!(r.method, "POST");
            assert_eq!(r.url, "https://api.x.dev/v1/users/:id");
            let ps: Vec<_> = r
                .parameters
                .iter()
                .map(|p| (p.name.as_str(), p.value.as_str()))
                .collect();
            assert_eq!(ps, [("id", ""), ("page", "2"), ("q", "a%20b")]);
            assert_eq!(r.headers.len(), 2);
            let b = r.body.unwrap();
            assert_eq!(
                (b.kind, b.text.as_str()),
                (BodyKind::Json, "{\"name\":\"it's\"}")
            );
        }
    }

    #[test]
    fn form_data_get_and_user() {
        let mut r = blank();
        assert!(apply(
            "curl -X put https://x.dev -d a=1 -d b=2 -u me:pw -L",
            &mut r
        ));
        assert_eq!(r.method, "PUT");
        let b = r.body.unwrap();
        assert_eq!(b.kind, BodyKind::FormUrlEncoded);
        assert_eq!(b.fields.unwrap().len(), 2);
        assert!(
            matches!(r.auth, AuthConfig::Basic { ref username, ref password, .. } if username == "me" && password == "pw")
        );

        let mut r = blank();
        assert!(apply("curl -G https://x.dev -d a=1 -A ua", &mut r));
        assert_eq!(
            (r.method.as_str(), r.parameters.len(), r.body),
            ("GET", 1, None)
        );
    }

    #[test]
    fn multipart_file_and_rejects() {
        let mut r = blank();
        assert!(apply(
            "curl https://x.dev -F 'doc=@/tmp/a.pdf;type=application/pdf' --form-string 'n=@x'",
            &mut r
        ));
        let f = r.body.unwrap().fields.unwrap();
        assert!(
            f[0].is_file
                && f[0].value == "/tmp/a.pdf"
                && f[0].content_type.as_deref() == Some("application/pdf")
        );
        assert!(!f[1].is_file && f[1].value == "@x");
        assert!(!apply("https://x.dev", &mut blank()));
        assert!(!apply("curl -L", &mut blank()));
    }
}
