//! Settings → mcp: how to plug an AI client into Yuki. The snippet carries this
//! binary's path (and `YUKI_DATA_DIR` when set) but no token: the `yuki mcp`
//! bridge reads it from the data dir.

use std::ops::Range;

use gpui_kit::component::{button::Button, h_flex, v_flex, ActiveTheme as _, Sizable as _};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{
    settings::{Section, SettingsScreen},
    settings_view::field,
    workspace::Yuki,
    OpenMcp,
};

#[derive(Clone, Copy, PartialEq, Default)]
pub enum Client {
    #[default]
    ClaudeCode,
    Codex,
    Omp,
    Pi,
    Cursor,
    ClaudeDesktop,
}

const CLIENTS: [(Client, &str); 6] = [
    (Client::ClaudeCode, "Claude Code"),
    (Client::Codex, "Codex"),
    (Client::Omp, "omp"),
    (Client::Pi, "pi"),
    (Client::Cursor, "Cursor"),
    (Client::ClaudeDesktop, "Claude Desktop"),
];

impl Client {
    /// Where the snippet goes, then how to check it took.
    fn steps(self) -> [String; 2] {
        let [a, b] = match self {
            Client::ClaudeCode | Client::Codex => [
                t!("run in a terminal:"),
                t!("in a new session, /mcp lists yuki."),
            ],
            Client::Omp => [
                t!("add to ~/.omp/agent/mcp.json (merge into mcpServers if the file exists):"),
                t!("in omp, /mcp list shows yuki."),
            ],
            Client::Pi => [
                t!("with pi-mcp-adapter installed (pi install npm:pi-mcp-adapter), add to ~/.pi/agent/mcp.json:"),
                t!("in pi, /mcp lists yuki."),
            ],
            Client::Cursor => [
                t!("add to ~/.cursor/mcp.json (merge into mcpServers if the file exists):"),
                t!("check that yuki is on in Cursor's MCP settings."),
            ],
            Client::ClaudeDesktop => [
                t!("Claude Desktop → settings → developer → edit config, and merge into claude_desktop_config.json:"),
                t!("restart Claude Desktop."),
            ],
        };
        [a.into_owned(), b.into_owned()]
    }

    fn snippet(self) -> String {
        let exe = std::env::current_exe()
            .and_then(|p| p.canonicalize())
            .map_or_else(
                |_| "/Applications/Yuki.app/Contents/MacOS/yuki".into(),
                |p| p.display().to_string(),
            );
        let data_dir = std::env::var("YUKI_DATA_DIR").ok();
        let env_flag = |flag: &str| {
            data_dir.as_deref().map_or(String::new(), |d| {
                format!(" {flag} {}", sh(&format!("YUKI_DATA_DIR={d}")))
            })
        };
        match self {
            Client::ClaudeCode => format!(
                "claude mcp add -s user yuki{} -- {} mcp",
                env_flag("-e"),
                sh(&exe)
            ),
            Client::Codex => format!(
                "codex mcp add yuki{} -- {} mcp",
                env_flag("--env"),
                sh(&exe)
            ),
            _ => {
                let mut server = serde_json::json!({ "command": exe, "args": ["mcp"] });
                if self == Client::Omp {
                    server["type"] = "stdio".into();
                }
                if let Some(d) = data_dir {
                    server["env"] = serde_json::json!({ "YUKI_DATA_DIR": d });
                }
                let config = serde_json::json!({ "mcpServers": { "yuki": server } });
                serde_json::to_string_pretty(&config).unwrap_or_default()
            }
        }
    }
}

/// Single-quoted for the shell unless it's plainly safe.
fn sh(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-=".contains(c))
    {
        s.into()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

#[derive(Clone, Copy)]
enum Tok {
    Cmd,
    Flag,
    Key,
    Str,
}

/// Colored spans of one snippet line: a shell command's program, flags and
/// path/env values, or JSON keys and strings.
fn tokens(line: &str, shell: bool) -> Vec<(Range<usize>, Tok)> {
    let mut out = Vec::new();
    if shell {
        let mut at = 0;
        for (i, word) in line.split(' ').enumerate() {
            let tok = if i == 0 {
                Some(Tok::Cmd)
            } else if word.starts_with('-') {
                Some(Tok::Flag)
            } else if word.contains(['/', '=']) {
                Some(Tok::Str)
            } else {
                None
            };
            out.extend(tok.map(|t| (at..at + word.len(), t)));
            at += word.len() + 1;
        }
        return out;
    }
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(open) = line[from..].find('"') {
        let start = from + open;
        let mut end = start + 1;
        while end < bytes.len() && bytes[end] != b'"' {
            end += if bytes[end] == b'\\' { 2 } else { 1 };
        }
        let end = (end + 1).min(line.len());
        let key = line[end..].trim_start().starts_with(':');
        out.push((start..end, if key { Tok::Key } else { Tok::Str }));
        from = end;
    }
    out
}

/// `OpenMcp` (the `● mcp` chip, the palette) lands on this section.
pub(crate) fn on_actions(el: Div, cx: &mut Context<Yuki>) -> Div {
    el.on_action(
        cx.listener(|this, _: &OpenMcp, window, cx| this.open_settings(Section::Mcp, window, cx)),
    )
}

impl SettingsScreen {
    pub(crate) fn render_mcp(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (fg, muted, active_bg, hover_bg, code_bg, radius) = (
            theme.foreground,
            theme.muted_foreground,
            theme.list_active,
            theme.list_hover,
            theme.secondary,
            theme.radius,
        );
        let (blue, green, magenta) = (theme.blue, theme.green, theme.magenta);
        let current = self.mcp_client;
        let clients = CLIENTS.map(|(client, label)| {
            let selected = client == current;
            div()
                .id(SharedString::from(format!("mcp-{label}")))
                .px_2()
                .py_0p5()
                .rounded_sm()
                .cursor_pointer()
                .text_color(if selected { fg } else { muted })
                .when(selected, |s| s.bg(active_bg))
                .hover(move |s| s.bg(hover_bg))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.mcp_client = client;
                    cx.notify();
                }))
        });
        let [before, after] = current.steps();
        let snippet = current.snippet();
        let shell = matches!(current, Client::ClaudeCode | Client::Codex);
        let lines = snippet.lines().map(|line| {
            let spans = tokens(line, shell).into_iter().map(|(range, tok)| {
                let color = match tok {
                    Tok::Cmd | Tok::Key => blue,
                    Tok::Flag => magenta,
                    Tok::Str => green,
                };
                let style = HighlightStyle {
                    color: Some(color),
                    ..Default::default()
                };
                (range, style)
            });
            div().child(StyledText::new(line.to_string()).with_highlights(spans))
        });
        let code = h_flex()
            .items_start()
            .gap_2()
            .p_2()
            .rounded(radius)
            .bg(code_bg)
            .text_xs()
            .child(v_flex().flex_1().min_w_0().children(lines))
            .child(
                Button::new("mcp-copy")
                    .xsmall()
                    .outline()
                    .label(t!("copy").into_owned())
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(snippet.clone()))
                    }),
            );
        let setup = v_flex()
            .gap_2()
            .child(format!("1. {before}"))
            .child(code)
            .child(format!("2. {after}"));
        v_flex()
            .gap_4()
            .child(field(
                t!("client").into_owned(),
                h_flex().gap_2().flex_wrap().children(clients),
                None,
                cx,
            ))
            .child(field(
                t!("setup").into_owned(),
                setup,
                Some(t!("Yuki has to stay open: the agent talks to this app. no token to copy — the bridge finds it on its own.").into_owned()),
                cx,
            ))
            .into_any_element()
    }
}
