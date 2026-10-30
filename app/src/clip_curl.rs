//! A `curl …` in the clipboard, seen when a window comes to the front, is
//! offered as `▸ import cURL` in the status line: one click makes it a new
//! request (where ⌘N would put it), via `paste_curl`. Each clipboard content is
//! offered once, across windows; Yuki's own "copy as cURL" never is.

use gpui_kit::*;
use yuki_core::HttpRequest;

use crate::{items::Op, paste_curl, workspace::Yuki};

/// Clipboard state already looked at, shared by every window.
#[derive(Default)]
struct Seen {
    change: isize,
    text: Option<String>,
}

impl Global for Seen {}

/// Bumped on every copy; reading it doesn't count as a paste for macOS's
/// pasteboard privacy, reading the text does — so read only when it moved.
fn change_count() -> isize {
    objc2_app_kit::NSPasteboard::generalPasteboard().changeCount()
}

/// What `apply` writes over; fields it doesn't touch come from the store.
fn scratch() -> HttpRequest {
    serde_json::from_value(serde_json::json!({
        "id": "", "type": "api", "model": "http_request", "workspaceId": "",
        "folderId": null, "method": "GET", "name": "", "url": "",
        "createdAt": "", "updatedAt": ""
    }))
    .expect("static request")
}

/// `/path` of the URL (host when there's none): the sidebar shows it next to
/// the method, like the requests Yuki imports.
fn name_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    match rest.find('/') {
        Some(i) if url.contains("://") => rest[i..].into(),
        _ => rest.into(),
    }
}

/// Our own copy: never offer it back.
pub(crate) fn mark_seen(text: &str, cx: &mut App) {
    let seen = cx.default_global::<Seen>();
    seen.change = change_count();
    seen.text = Some(text.into());
}

impl Yuki {
    /// On window activation: offer the clipboard's cURL if it's new.
    pub(crate) fn check_clipboard(&mut self, cx: &mut Context<Self>) {
        let change = change_count();
        if cx.default_global::<Seen>().change == change {
            return;
        }
        let text = cx.read_from_clipboard().and_then(|c| c.text());
        let seen = cx.default_global::<Seen>();
        seen.change = change;
        if text.is_none() || seen.text == text {
            return;
        }
        seen.text = text.clone();
        let curl = text.filter(|t| {
            t.trim_start().starts_with("curl ") && paste_curl::apply(t, &mut scratch())
        });
        if curl.is_some() || self.clip_curl.is_some() {
            self.clip_curl = curl;
            cx.notify();
        }
    }

    pub(crate) fn dismiss_clip_curl(&mut self, cx: &mut Context<Self>) {
        self.clip_curl = None;
        cx.notify();
    }

    /// New request from the offered cURL, opened like ⌘N's.
    pub(crate) fn import_clip_curl(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(cmd) = self.clip_curl.take() else {
            return;
        };
        let parent = self.active_folder(cx);
        let backend = self.backend.clone();
        let op: Op = Box::new(move |s, ws| {
            let mut req = scratch();
            paste_curl::apply(&cmd, &mut req);
            let name = name_of(&req.url);
            let mut created = s.create_request(ws.into(), parent, name, req.method, req.url)?;
            created.parameters = req.parameters;
            created.headers = req.headers;
            created.body = req.body;
            created.auth = req.auth;
            // Seals basic-auth passwords on encrypted workspaces.
            backend.save(&created)?;
            Ok(Some(created.id))
        });
        self.mutate(false, op, window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::name_of;

    #[test]
    fn names_are_the_path() {
        assert_eq!(
            name_of("https://api.x.com/gw/logo/upload"),
            "/gw/logo/upload"
        );
        assert_eq!(name_of("https://api.x.com"), "api.x.com");
        assert_eq!(name_of("{{ BASE }}/users"), "{{ BASE }}/users");
        assert_eq!(name_of("localhost:3000/a"), "localhost:3000/a");
    }
}
