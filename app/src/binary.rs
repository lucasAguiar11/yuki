//! Binary response body: images and PDFs render inline, everything else gets
//! a card; all of them can be saved or opened in the system's default app.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use gpui_kit::component::{button::Button, h_flex, v_flex, ActiveTheme as _, Sizable as _};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_core::HttpResponse;

use crate::{editor::RequestEditor, ui};

/// PDF pages rendered; past it, `open` has the rest.
const MAX_PAGES: usize = 50;

/// A binary body decoded once for display (off the UI thread).
pub enum Media {
    Image(Arc<Image>),
    /// Rendered pages and their size in points.
    Pdf(Vec<(Arc<RenderImage>, f32)>),
}

fn media(content_type: &str, bytes: Vec<u8>) -> Option<Media> {
    if content_type == "application/pdf" {
        #[cfg(target_os = "macos")]
        {
            let pages = crate::pdf::pages(&bytes, 2.0, MAX_PAGES);
            return (!pages.is_empty()).then(|| {
                Media::Pdf(
                    pages
                        .into_iter()
                        .map(|p| (Arc::new(RenderImage::new(vec![p.frame])), p.width))
                        .collect(),
                )
            });
        }
    }
    let format = ImageFormat::from_mime_type(content_type)?;
    Some(Media::Image(Arc::new(Image::from_bytes(format, bytes))))
}

fn header<'a>(resp: &'a HttpResponse, name: &str) -> Option<&'a str> {
    resp.headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

pub(crate) fn content_type(resp: &HttpResponse) -> &str {
    header(resp, "content-type")
        .and_then(|v| v.split(';').next())
        .map_or("", str::trim)
}

/// The server's `Content-Disposition` name, else the URL's last segment when
/// it looks like a file (`…/dados.csv`), else `response.<ext>` from the content
/// type. Only the last path component: neither can pick the folder.
pub fn file_name(resp: &HttpResponse, url: &str) -> String {
    let base = |s: &str| {
        let name = Path::new(s.trim().trim_matches('"')).file_name()?;
        Some(name.to_string_lossy().into_owned())
    };
    let given = header(resp, "content-disposition").and_then(|v| {
        base(
            v.split(';')
                .find_map(|p| p.trim().strip_prefix("filename="))?,
        )
    });
    let from_url = || {
        let rest = url.split(['?', '#']).next()?;
        let path = rest
            .split_once("://")
            .map_or(rest, |(_, r)| r)
            .split_once('/')?
            .1;
        base(path.rsplit('/').next()?).filter(|n| n.contains('.'))
    };
    given.or_else(from_url).unwrap_or_else(|| {
        let ext = mime_guess::get_mime_extensions_str(content_type(resp))
            .and_then(|e| e.first())
            .unwrap_or(&"bin");
        format!("response.{ext}")
    })
}

fn sent_url(editor: &RequestEditor) -> &str {
    editor.sent.as_ref().map_or("", |r| r.url.as_str())
}

/// The body as the server sent it: text verbatim, binary from its base64.
fn bytes(resp: &HttpResponse) -> Result<Vec<u8>, String> {
    if resp.body_is_text {
        return Ok(resp.body.clone().into_bytes());
    }
    base64::engine::general_purpose::STANDARD
        .decode(&resp.body)
        .map_err(|e| e.to_string())
}

impl RequestEditor {
    /// Decodes the shown response's image/PDF; a newer response drops the task.
    pub(crate) fn load_media(&mut self, cx: &mut Context<Self>) {
        self.media = None;
        self.media_task = None;
        let Some(Ok(resp)) = &self.response else {
            return;
        };
        if resp.body_is_text {
            return;
        }
        let (ct, body) = (content_type(resp).to_string(), resp.body.clone());
        let build = cx.background_spawn(async move {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(body)
                .ok()?;
            media(&ct, bytes)
        });
        self.media_task = Some(cx.spawn(async move |this, cx| {
            let media = build.await;
            _ = this.update(cx, |this, cx| {
                this.media = media;
                cx.notify();
            });
        }));
    }

    fn binary(&self) -> Option<(String, Result<Vec<u8>, String>)> {
        let Some(Ok(resp)) = &self.response else {
            return None;
        };
        Some((file_name(resp, sent_url(self)), bytes(resp)))
    }

    /// Body to a file the user picks (any response, text or binary).
    pub(crate) fn download(&mut self, cx: &mut Context<Self>) {
        let Some((name, data)) = self.binary() else {
            return;
        };
        let dir = dirs::download_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        let path = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = path.await else {
                return;
            };
            let write = cx.background_spawn(async move {
                std::fs::write(&path, data?).map_err(|e| e.to_string())?;
                Ok::<_, String>(path)
            });
            let result = write.await;
            _ = this.update(cx, |this, cx| {
                this.file_note = Some(match result {
                    Ok(path) => {
                        cx.reveal_path(&path);
                        t!("saved to %{path}", path = path.display())
                            .into_owned()
                            .into()
                    }
                    Err(e) => t!("can't save: %{e}", e = e).into_owned().into(),
                });
                cx.notify();
            });
        })
        .detach();
    }

    /// Into the temp dir under its own name, then the default app (Preview for a PDF).
    fn open_binary(&mut self, cx: &mut Context<Self>) {
        let Some((name, data)) = self.binary() else {
            return;
        };
        let path = std::env::temp_dir().join("yuki").join(name);
        let write = cx.background_spawn(async move {
            std::fs::create_dir_all(path.parent().unwrap_or(&path)).map_err(|e| e.to_string())?;
            std::fs::write(&path, data?).map_err(|e| e.to_string())?;
            Ok::<_, String>(path)
        });
        cx.spawn(async move |this, cx| {
            let result = write.await;
            _ = this.update(cx, |this, cx| {
                match result {
                    Ok(path) => {
                        cx.open_with_system(&path);
                        this.file_note = None;
                    }
                    Err(e) => {
                        this.file_note = Some(t!("can't open: %{e}", e = e).into_owned().into())
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// Inline image/PDF under a `type · size · name   open` bar; anything
/// else (or still decoding) gets the same facts as a centered card.
/// `download…` lives in the status strip, for every response.
pub fn render(
    editor: &RequestEditor,
    resp: &HttpResponse,
    cx: &mut Context<RequestEditor>,
) -> AnyElement {
    let open = Button::new("binary-open")
        .small()
        .outline()
        .label(t!("open").into_owned())
        .on_click(cx.listener(|this, _, _, cx| this.open_binary(cx)));
    let theme = cx.theme();
    let (fg, muted, border, mono) = (
        theme.foreground,
        theme.muted_foreground,
        theme.border,
        theme.mono_font_family.clone(),
    );
    let kind = match content_type(resp) {
        "" => t!("binary").into_owned(),
        ct => ct.to_string(),
    };
    let facts = format!("{kind} · {}", ui::human_bytes(resp.body_size));
    let Some(media) = &editor.media else {
        return v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .text_sm()
            .font_family(mono)
            .child(div().text_color(fg).child(facts))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(file_name(resp, sent_url(editor))),
            )
            .child(open)
            .into_any_element();
    };
    let bar = h_flex()
        .px_4()
        .py_1p5()
        .gap_3()
        .text_xs()
        .font_family(mono)
        .text_color(muted)
        .border_b_1()
        .border_color(border)
        .child(div().text_color(fg).child(facts))
        .child(file_name(resp, sent_url(editor)))
        .child(div().flex_1())
        .child(open);
    let content = match media {
        Media::Image(image) => div()
            .flex()
            .justify_center()
            .child(img(image.clone()).max_w_full()),
        Media::Pdf(pages) => v_flex().items_center().gap_4().children(
            pages
                .iter()
                .map(|(page, width)| img(page.clone()).w(px(*width)).max_w_full()),
        ),
    };
    v_flex()
        .size_full()
        .child(bar)
        .child(
            div()
                .id("binary-media")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_4()
                .when(
                    matches!(media, Media::Pdf(p) if p.len() >= MAX_PAGES),
                    |s| {
                        s.child(
                            div().pb_2().text_xs().text_color(muted).child(
                                t!("first %{n} pages — open shows them all", n = MAX_PAGES)
                                    .into_owned(),
                            ),
                        )
                    },
                )
                .child(content),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{file_name, HttpResponse};

    fn resp(headers: &[(&str, &str)]) -> HttpResponse {
        let headers: Vec<_> = headers
            .iter()
            .map(|(n, v)| serde_json::json!({"name": n, "value": v}))
            .collect();
        serde_json::from_value(serde_json::json!({
            "requestId": "", "status": 200, "statusText": "OK", "headers": headers,
            "body": "", "bodySize": 0, "bodyIsText": false
        }))
        .unwrap()
    }

    #[test]
    fn names_the_file() {
        let pdf = resp(&[("Content-Type", "application/pdf")]);
        assert_eq!(file_name(&pdf, "http://h/api/pre-alert"), "response.pdf");
        assert_eq!(file_name(&pdf, "http://h/f/dados.csv?x=1"), "dados.csv");
        let given = resp(&[(
            "Content-Disposition",
            "attachment; filename=\"../../x/pre-alerta.pdf\"",
        )]);
        assert_eq!(file_name(&given, "http://h/a.bin"), "pre-alerta.pdf");
        assert_eq!(file_name(&resp(&[]), "http://api.com"), "response.bin");
    }
}
