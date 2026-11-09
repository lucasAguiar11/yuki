//! Settings → workspace → client certificate: the PEM files sends present
//! when a server asks for one (mTLS). Machine-local, written on pick.

use gpui_kit::component::{button::Button, h_flex, v_flex, ActiveTheme as _, Sizable as _};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_mcp::resolve::{self, ClientCert};

use crate::{settings::SettingsScreen, settings_view::field, ui};

impl SettingsScreen {
    /// `key`: pick the key file (the certificate must be set already).
    fn pick_client_file(&mut self, key: bool, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            _ = this.update(cx, |this, cx| {
                let cert = match (this.client_cert.take(), key) {
                    (Some(c), true) => ClientCert {
                        key: Some(path),
                        ..c
                    },
                    (Some(c), false) => ClientCert { cert: path, ..c },
                    (None, _) => ClientCert {
                        cert: path,
                        key: None,
                    },
                };
                this.set_client_cert(Some(cert), cx);
            });
        })
        .detach();
    }

    fn set_client_cert(&mut self, cert: Option<ClientCert>, cx: &mut Context<Self>) {
        self.client_cert = cert.clone();
        self.write_local(
            move |id, dir| resolve::set_client_cert(id, dir, cert.as_ref()),
            cx,
        );
    }

    pub(crate) fn render_client_cert(&self, cx: &mut Context<Self>) -> Div {
        let (fg, muted) = (cx.theme().foreground, ui::faint(cx));
        let row = |id: &'static str,
                   label: String,
                   path: Option<String>,
                   key: bool,
                   cx: &mut Context<Self>| {
            let empty = path.is_none();
            h_flex()
                .gap_3()
                .child(div().w(px(32.)).flex_none().text_color(muted).child(label))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(if empty { muted } else { fg })
                        .child(path.unwrap_or_else(|| t!("none").into_owned())),
                )
                .child(
                    Button::new(id)
                        .xsmall()
                        .outline()
                        .label(t!("choose…").into_owned())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.pick_client_file(key, window, cx)
                        })),
                )
        };
        let shown = |p: &std::path::Path| p.display().to_string();
        let cert = self.client_cert.as_ref();
        let control = v_flex()
            .gap_1()
            .text_xs()
            .child(row(
                "cert-file",
                t!("cert").into_owned(),
                cert.map(|c| shown(&c.cert)),
                false,
                cx,
            ))
            .when_some(cert, |s, c| {
                let key = c
                    .key
                    .as_deref()
                    .map(shown)
                    .unwrap_or_else(|| t!("in the certificate file").into_owned());
                s.child(row("cert-key", t!("key").into_owned(), Some(key), true, cx))
                    .child(
                        h_flex().child(
                            Button::new("cert-remove")
                                .xsmall()
                                .outline()
                                .label(t!("remove").into_owned())
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.set_client_cert(None, cx)),
                                ),
                        ),
                    )
            });
        field(
            t!("client certificate").into_owned(),
            control,
            Some(
                t!("PEM files sent when a server asks for one (mTLS). this machine only.")
                    .into_owned(),
            ),
            cx,
        )
    }
}
