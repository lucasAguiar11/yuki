//! The query lives in the request's `parameters` (the params tab), not in the
//! URL bar: a URL pasted or typed with `?a=1&b=2` hands its pairs to the rows.
//! The engine builds the query from the enabled non-path params at send.

use yuki_core::{new_id, HttpRequest, RequestParameter};
use yuki_mcp::resolve::path_params;

/// `name=value` pairs of the URL's query, in order; a bare `flag` has an empty
/// value. Kept as typed (no percent-decoding). The fragment isn't part of it.
pub fn pairs(url: &str) -> Vec<(String, String)> {
    let Some(start) = url.find('?').map(|i| i + 1) else {
        return Vec::new();
    };
    let end = url[start..].find('#').map_or(url.len(), |i| start + i);
    url[start..end]
        .split('&')
        .filter(|s| !s.is_empty())
        .map(|s| match s.split_once('=') {
            Some((k, v)) => (k.to_string(), v.to_string()),
            None => (s.to_string(), String::new()),
        })
        .collect()
}

/// `url` without its query; the fragment stays.
pub fn without_query(url: &str) -> String {
    let fragment = url.find('#').map_or("", |i| &url[i..]);
    let base = &url[..url.find(['?', '#']).unwrap_or(url.len())];
    format!("{base}{fragment}")
}

pub fn as_params(pairs: Vec<(String, String)>) -> Vec<RequestParameter> {
    pairs
        .into_iter()
        .map(|(name, value)| RequestParameter {
            id: new_id(),
            name,
            value,
            enabled: true,
        })
        .collect()
}

/// The editor's form of a stored request: no query in the URL, path params
/// first, then query params. A query still in the URL (Yuki used to keep it
/// there) becomes params — unless params already hold the query: then the
/// engine never sent the URL's one, so it's dropped.
pub fn normalize(req: &mut HttpRequest) {
    let path = path_params(&req.url);
    let (path_ps, query_ps): (Vec<_>, Vec<_>) = std::mem::take(&mut req.parameters)
        .into_iter()
        .partition(|p| path.contains(&p.name));
    let from_url = if query_ps.is_empty() {
        as_params(pairs(&req.url))
    } else {
        Vec::new()
    };
    req.url = without_query(&req.url);
    req.parameters = path_ps
        .into_iter()
        .chain(query_ps)
        .chain(from_url)
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(k: &str, v: &str) -> (String, String) {
        (k.into(), v.into())
    }

    #[test]
    fn splits_the_query() {
        let url = "http://h/saida/saldo?cnpj_cliente=16810540009399&cod_produto=7014EA08AAC0001";
        assert_eq!(
            pairs(url),
            [
                p("cnpj_cliente", "16810540009399"),
                p("cod_produto", "7014EA08AAC0001")
            ]
        );
        assert_eq!(without_query(url), "http://h/saida/saldo");
        let tpl = "{{ BASE }}/x?id={{ $uuid.v4() }}&flag&a=b=c#top";
        assert_eq!(
            pairs(tpl),
            [p("id", "{{ $uuid.v4() }}"), p("flag", ""), p("a", "b=c")]
        );
        assert_eq!(without_query(tpl), "{{ BASE }}/x#top");
        assert!(pairs("http://h/x").is_empty());
    }

    fn param(name: &str, value: &str) -> RequestParameter {
        RequestParameter {
            id: name.into(),
            name: name.into(),
            value: value.into(),
            enabled: true,
        }
    }

    fn names(req: &HttpRequest) -> Vec<(&str, &str)> {
        req.parameters
            .iter()
            .map(|p| (p.name.as_str(), p.value.as_str()))
            .collect()
    }

    #[test]
    fn normalize_moves_the_query_into_params() {
        let mut req: HttpRequest = serde_json::from_value(serde_json::json!({
            "id": "r", "type": "api", "model": "http_request", "workspaceId": "w",
            "method": "GET", "name": "r", "url": "{{ BASE }}/u/:id?a=1&b",
            "parameters": [{"id": "x", "name": "id", "value": "7", "enabled": true}],
            "headers": [], "order": 0.0, "createdAt": "", "updatedAt": ""
        }))
        .unwrap();
        normalize(&mut req);
        assert_eq!(req.url, "{{ BASE }}/u/:id");
        assert_eq!(names(&req), [("id", "7"), ("a", "1"), ("b", "")]);

        // Params already holding the query win: the URL's was never sent.
        req.url = "{{ BASE }}/u/:id?stale=1".into();
        req.parameters = vec![param("q", "2"), param("id", "7")];
        normalize(&mut req);
        assert_eq!(names(&req), [("id", "7"), ("q", "2")]);
    }
}
