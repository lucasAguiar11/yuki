//! Indented XML for the response viewer and the request body's `format`.
//! Tags are kept byte for byte (no parse/re-serialize: entities and quoting
//! stay as written); only whitespace between tags changes, and in the viewer
//! the whitespace around text.

use yuki_mcp::resolve::{xml_tokens as tokens, XmlTok as Tok};

/// `text` with one element per line, two spaces per level; a leaf stays on one
/// line (`<a>1</a>`). `None` when it isn't balanced XML (shown as received).
///
/// `keep_text` is for a body that gets sent: text and CDATA stay exactly as
/// written, so text beside elements (no indenting it without changing it) is
/// `None` too. Blank text between tags still goes, as in `compact_xml`.
pub fn pretty_xml(text: &str, keep_text: bool) -> Option<String> {
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
    let kept =
        |t: &Tok| keep_text && (matches!(t, Tok::Text(_)) || t.as_str().starts_with("<![CDATA["));
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut depth = 0usize;
    let mut ix = 0;
    while ix < toks.len() {
        match (&toks[ix], toks.get(ix + 1), toks.get(ix + 2)) {
            (Tok::Open(o), Some(t), Some(Tok::Close(c)))
                if matches!(t, Tok::Text(_)) || kept(t) =>
            {
                let t = t.as_str();
                let t = if keep_text { t } else { t.trim() };
                line(depth, &format!("{o}{t}{c}"), &mut out);
                ix += 3;
            }
            (t, _, _) if kept(t) => return None,
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
        assert_eq!(pretty_xml(src, false).as_deref(), Some(want));
        // Already indented: same result.
        assert_eq!(pretty_xml(want, false).as_deref(), Some(want));
        for bad in ["<a><b></a>", "<a>", "</a>", "<a x=\"1>", "plain text"] {
            assert_eq!(pretty_xml(bad, false), None, "{bad}");
        }
    }

    #[test]
    fn keep_text_never_changes_what_is_sent() {
        let src = "<a><b> x </b><c><![CDATA[ y ]]></c><d>\n</d></a>";
        let want = "<a>\n  <b> x </b>\n  <c><![CDATA[ y ]]></c>\n  <d></d>\n</a>";
        assert_eq!(pretty_xml(src, true).as_deref(), Some(want));
        assert_eq!(
            pretty_xml(src, false).unwrap().lines().nth(1),
            Some("  <b>x</b>")
        );
        for mixed in ["<p>a <b>b</b> c</p>", "<a><b/><![CDATA[x]]></a>"] {
            assert_eq!(pretty_xml(mixed, true), None, "{mixed}");
        }
    }
}
