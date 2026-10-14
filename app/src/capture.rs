//! Response field → variable, pure part: the JSONPath under the caret, a
//! suggested name, the `$response.body` formula, and which variables read a
//! request. UI in `capture_view.rs`.

use yuki_mcp::resolve::{funcs, is_identifier};

use crate::vars::Vars;

/// A variable that reads this request: name and JSONPath.
pub struct Reader {
    pub name: String,
    pub selector: String,
}

enum Frame {
    Obj(Option<String>),
    Arr(usize),
}

/// JSONPath of the value (or key) under `offset` in `text`, `None` when the
/// caret sits on punctuation or whitespace. Containers hit on their opening
/// bracket.
pub fn json_path_at(text: &str, offset: usize) -> Option<String> {
    let b = text.as_bytes();
    let mut stack: Vec<Frame> = Vec::new();
    let mut expect_key = false;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            c @ (b'{' | b'[') => {
                if offset == i {
                    return Some(path(&stack));
                }
                stack.push(if c == b'{' {
                    Frame::Obj(None)
                } else {
                    Frame::Arr(0)
                });
                expect_key = c == b'{';
                i += 1;
            }
            b'}' | b']' => {
                stack.pop();
                i += 1;
            }
            b',' => {
                match stack.last_mut() {
                    Some(Frame::Obj(key)) => {
                        *key = None;
                        expect_key = true;
                    }
                    Some(Frame::Arr(ix)) => *ix += 1,
                    None => {}
                }
                i += 1;
            }
            b':' => {
                expect_key = false;
                i += 1;
            }
            b'"' => {
                let mut end = i + 1;
                while end < b.len() && b[end] != b'"' {
                    end += if b[end] == b'\\' { 2 } else { 1 };
                }
                let end = end.min(b.len() - 1);
                if expect_key {
                    if let Some(Frame::Obj(key)) = stack.last_mut() {
                        *key = serde_json::from_str(&text[i..=end]).ok();
                    }
                }
                if (i..=end + 1).contains(&offset) {
                    return Some(path(&stack));
                }
                i = end + 1;
            }
            c if c.is_ascii_whitespace() => i += 1,
            _ => {
                let end = text[i..]
                    .find(|c: char| matches!(c, ',' | '}' | ']') || c.is_whitespace())
                    .map_or(b.len(), |e| i + e);
                if (i..=end).contains(&offset) {
                    return Some(path(&stack));
                }
                i = end;
            }
        }
    }
    None
}

fn path(stack: &[Frame]) -> String {
    let mut out = String::from("$");
    for frame in stack {
        match frame {
            Frame::Obj(Some(k)) if is_identifier(k) => out += &format!(".{k}"),
            Frame::Obj(Some(k)) => out += &format!("['{}']", k.replace('\'', "\\'")),
            Frame::Obj(None) => {}
            Frame::Arr(ix) => out += &format!("[{ix}]"),
        }
    }
    out
}

/// `$.data.accessToken` → `ACCESS_TOKEN`: the last key in upper snake case.
pub fn suggest_name(path: &str) -> String {
    let last = path
        .rsplit(['.', '['])
        .map(|s| s.trim_matches(|c| matches!(c, '\'' | ']' | '$')))
        .find(|s| !s.is_empty() && !s.chars().all(|c| c.is_ascii_digit()))
        .unwrap_or("value");
    let mut out = String::new();
    let mut prev_lower = false;
    for c in last.chars() {
        if c.is_uppercase() && prev_lower {
            out.push('_');
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        out.push(if c.is_alphanumeric() { c } else { '_' });
    }
    let out = out.to_uppercase();
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{out}")
    } else {
        out
    }
}

fn quote(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub fn formula(req_id: &str, path: &str) -> String {
    format!(
        "{{{{ $response.body(requestId=\"{}\", selector=\"{}\") }}}}",
        quote(req_id),
        quote(path)
    )
}

/// Variables whose value reads `req_id`'s response.
pub fn readers(vars: &Vars, req_id: &str) -> Vec<Reader> {
    vars.iter()
        .filter_map(|(name, var)| {
            let inner = var
                .raw
                .trim()
                .strip_prefix("{{")?
                .strip_suffix("}}")?
                .trim();
            let (fn_name, args) = funcs::call(inner.strip_prefix('$')?)?;
            let arg = |k: &str| args.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone());
            (fn_name == "response.body" && arg("requestId")? == req_id).then(|| Reader {
                name: name.clone(),
                selector: arg("selector").unwrap_or_default(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{json_path_at, suggest_name};

    #[test]
    fn path_at_caret() {
        let text = r#"{
  "data": {
    "access_token": "abc",
    "list": [1, {"id": 7}]
  },
  "a b": true
}"#;
        let at = |needle: &str| text.find(needle).unwrap();
        assert_eq!(
            json_path_at(text, at("\"abc\"") + 1).as_deref(),
            Some("$.data.access_token")
        );
        assert_eq!(
            json_path_at(text, at("access_token") + 2).as_deref(),
            Some("$.data.access_token")
        );
        assert_eq!(
            json_path_at(text, at("7")).as_deref(),
            Some("$.data.list[1].id")
        );
        assert_eq!(
            json_path_at(text, at("1,")).as_deref(),
            Some("$.data.list[0]")
        );
        assert_eq!(
            json_path_at(text, at("true") + 2).as_deref(),
            Some("$['a b']")
        );
        assert_eq!(json_path_at(text, at("[1")).as_deref(), Some("$.data.list"));
        assert_eq!(json_path_at(text, 1), None);
    }

    #[test]
    fn names_from_paths() {
        assert_eq!(suggest_name("$.data.access_token"), "ACCESS_TOKEN");
        assert_eq!(suggest_name("$.data.accessToken"), "ACCESS_TOKEN");
        assert_eq!(suggest_name("$.items[0]"), "ITEMS");
        assert_eq!(suggest_name("$['a b']"), "A_B");
        assert_eq!(suggest_name("$"), "VALUE");
    }
}
