//! Indented XML for the response viewer. Text and attributes are kept byte for
//! byte (no parse/re-serialize: entities and quoting stay as received); only
//! whitespace between tags changes.

enum Tok<'a> {
    Open(&'a str),
    Close(&'a str),
    /// Self-closing tags, `<?…?>`, comments, CDATA, doctype.
    Lone(&'a str),
    Text(&'a str),
}

fn tokens(s: &str) -> Option<Vec<Tok<'_>>> {
    let mut out = vec![];
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        if !rest.starts_with('<') {
            let end = rest.find('<').unwrap_or(rest.len());
            out.push(Tok::Text(&rest[..end]));
            i += end;
            continue;
        }
        let close = |pat: &str| rest.find(pat).map(|e| e + pat.len());
        let len = if rest.starts_with("<!--") {
            close("-->")?
        } else if rest.starts_with("<![CDATA[") {
            close("]]>")?
        } else if rest.starts_with("<?") {
            close("?>")?
        } else {
            // `>` inside a quoted attribute value doesn't end the tag.
            let mut quote = None;
            rest.char_indices().find_map(|(ix, c)| match (quote, c) {
                (None, '"' | '\'') => {
                    quote = Some(c);
                    None
                }
                (Some(q), c) if c == q => {
                    quote = None;
                    None
                }
                (None, '>') => Some(ix + 1),
                _ => None,
            })?
        };
        let tag = &rest[..len];
        out.push(if tag.starts_with("</") {
            Tok::Close(tag)
        } else if tag.ends_with("/>") || tag.starts_with("<!") || tag.starts_with("<?") {
            Tok::Lone(tag)
        } else {
            Tok::Open(tag)
        });
        i += len;
    }
    Some(out)
}

/// `text` with one element per line, two spaces per level; a leaf stays on one
/// line (`<a>1</a>`). `None` when it isn't balanced XML (shown as received).
pub fn pretty_xml(text: &str) -> Option<String> {
    let toks: Vec<Tok> = tokens(text.trim())?
        .into_iter()
        .filter(|t| !matches!(t, Tok::Text(s) if s.trim().is_empty()))
        .collect();
    if !toks
        .iter()
        .any(|t| matches!(t, Tok::Open(_) | Tok::Lone(_)))
    {
        return None;
    }
    fn line(depth: usize, s: &str, out: &mut String) {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&"  ".repeat(depth));
        out.push_str(s);
    }
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut depth = 0usize;
    let mut ix = 0;
    while ix < toks.len() {
        match (&toks[ix], toks.get(ix + 1), toks.get(ix + 2)) {
            (Tok::Open(o), Some(Tok::Text(t)), Some(Tok::Close(c))) => {
                line(depth, &format!("{o}{}{c}", t.trim()), &mut out);
                ix += 3;
            }
            (Tok::Open(o), Some(Tok::Close(c)), _) => {
                line(depth, &format!("{o}{c}"), &mut out);
                ix += 2;
            }
            (Tok::Open(o), _, _) => {
                line(depth, o, &mut out);
                depth += 1;
                ix += 1;
            }
            (Tok::Close(c), _, _) => {
                depth = depth.checked_sub(1)?;
                line(depth, c, &mut out);
                ix += 1;
            }
            (Tok::Lone(s), _, _) => {
                line(depth, s, &mut out);
                ix += 1;
            }
            (Tok::Text(t), _, _) => {
                line(depth, t.trim(), &mut out);
                ix += 1;
            }
        }
    }
    (depth == 0 && !out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::pretty_xml;

    #[test]
    fn indents_elements_and_keeps_content() {
        let src = r#"<?xml version="1.0"?><a x="1>2"><!-- c --><b>t &amp; u</b><c/><d><e></e><![CDATA[<raw>]]></d></a>"#;
        let want = "<?xml version=\"1.0\"?>\n<a x=\"1>2\">\n  <!-- c -->\n  <b>t &amp; u</b>\n  <c/>\n  <d>\n    <e></e>\n    <![CDATA[<raw>]]>\n  </d>\n</a>";
        assert_eq!(pretty_xml(src).as_deref(), Some(want));
        // Already indented: same result.
        assert_eq!(pretty_xml(want).as_deref(), Some(want));
        for bad in ["<a><b></a>", "<a>", "</a>", "<a x=\"1>", "plain text"] {
            assert_eq!(pretty_xml(bad), None, "{bad}");
        }
    }
}
