//! Settings → workspace → client certificates: the PEM files sends present
//! when a server asks for one (mTLS), one row per host (empty host = any
//! other). Machine-local; saved with the screen's autosave.

use std::path::Path;

use gpui_kit::component::{
    h_flex,
    input::{Input, InputEvent, InputState},
    tooltip::Tooltip,
    v_flex, ActiveTheme as _, Sizable as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_mcp::resolve::ClientCert;

use crate::{settings::SettingsScreen, settings_view::field, ui};

/// One certificate as edited: its files, and the host as typed.
pub struct CertRow {
    files: ClientCert,
    host: Entity<InputState>,
    _sub: Subscription,
}

impl CertRow {
    pub fn new(cert: ClientCert, window: &mut Window, cx: &mut Context<SettingsScreen>) -> Self {
        let host = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("any host").into_owned())
                .default_value(cert.host.clone())
        });
        let _sub = cx.subscribe(&host, |this, _, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                this.changed(cx);
            }
        });
        Self {
            files: cert,
            host,
            _sub,
        }
    }

    pub fn draft(&self, cx: &App) -> ClientCert {
        ClientCert {
            host: self.host.read(cx).value().trim().to_string(),
            ..self.files.clone()
        }
    }
}

/// Which file a pick sets.
#[derive(Clone, Copy)]
enum Pick {
    NewRow,
    Cert(usize),
    Key(usize),
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into())
}

impl SettingsScreen {
    fn pick_cert_file(&mut self, pick: Pick, window: &mut Window, cx: &mut Context<Self>) {
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
            _ = this.update_in(cx, |this, window, cx| {
                match pick {
                    Pick::NewRow => {
                        let cert = ClientCert {
                            cert: path,
                            ..Default::default()
                        };
                        let row = CertRow::new(cert, window, cx);
                        this.client_certs.push(row);
                    }
                    Pick::Cert(ix) | Pick::Key(ix) => {
                        let Some(row) = this.client_certs.get_mut(ix) else {
                            return;
                        };
                        match pick {
                            Pick::Key(_) => row.files.key = Some(path),
                            _ => row.files.cert = path,
                        }
                    }
                }
                this.changed(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn remove_cert(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.client_certs.len() {
            self.client_certs.remove(ix);
            self.changed(cx);
            cx.notify();
        }
    }

    /// `host · certificate · key ×` rows, like the request tables, then
    /// `+ add certificate`.
    pub(crate) fn render_client_certs(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (fg, cyan, red, border) = (theme.foreground, theme.cyan, theme.red, theme.border);
        let faint = ui::faint(cx);
        // The file name (full path on hover), then `choose`, like `table.rs`.
        let file = |ids: (&'static str, &'static str),
                    ix: usize,
                    path: Option<&Path>,
                    empty: String,
                    pick: Pick| {
            let full: Option<SharedString> = path.map(|p| p.display().to_string().into());
            h_flex()
                .flex_1()
                .min_w_0()
                .gap_2()
                .child(
                    div()
                        .id((ids.0, ix))
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(if path.is_some() { cyan } else { faint })
                        .child(path.map_or(empty, file_name))
                        .when_some(full, |d, full| {
                            d.tooltip(move |window, cx| {
                                Tooltip::new(full.clone()).build(window, cx)
                            })
                        }),
                )
                .child(
                    div()
                        .id((ids.1, ix))
                        .cursor_pointer()
                        .text_color(faint)
                        .hover(|s| s.text_color(fg))
                        .child(t!("choose"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.pick_cert_file(pick, window, cx)
                        })),
                )
        };
        let head = h_flex()
            .h(px(24.))
            .px_2p5()
            .gap_3()
            .text_color(faint)
            .border_b_1()
            .border_color(border)
            .child(div().w(relative(0.34)).child(t!("host")))
            .child(div().flex_1().child(t!("certificate")))
            .child(div().flex_1().child(t!("key")))
            .child(div().w(px(12.)));
        let rows = self.client_certs.iter().enumerate().map(|(ix, row)| {
            let files = &row.files;
            h_flex()
                .group("cert-row")
                .h(px(28.))
                .px_2p5()
                .gap_3()
                .border_b_1()
                .border_color(border.opacity(0.5))
                .child(
                    div()
                        .w(relative(0.34))
                        .child(Input::new(&row.host).appearance(false).xsmall()),
                )
                .child(file(
                    ("cert-file", "cert-file-pick"),
                    ix,
                    Some(&files.cert),
                    String::new(),
                    Pick::Cert(ix),
                ))
                .child(file(
                    ("cert-key", "cert-key-pick"),
                    ix,
                    files.key.as_deref(),
                    t!("in the certificate").into_owned(),
                    Pick::Key(ix),
                ))
                .child(
                    div()
                        .id(("cert-rm", ix))
                        .w(px(12.))
                        .flex_none()
                        .cursor_pointer()
                        .text_color(faint)
                        .opacity(0.)
                        .group_hover("cert-row", |s| s.opacity(1.))
                        .hover(|s| s.text_color(red))
                        .child("×")
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_cert(ix, cx))),
                )
        });
        let add = div()
            .id("cert-add")
            .h(px(28.))
            .px_2p5()
            .flex()
            .items_center()
            .cursor_pointer()
            .text_color(faint)
            .hover(|s| s.text_color(fg))
            .child(t!("+ add certificate"))
            .on_click(
                cx.listener(|this, _, window, cx| this.pick_cert_file(Pick::NewRow, window, cx)),
            );
        let control = v_flex()
            .text_xs()
            .font_family(theme.mono_font_family.clone())
            .border_1()
            .border_color(border)
            .rounded(theme.radius)
            .when(!self.client_certs.is_empty(), |t| t.child(head))
            .children(rows)
            .child(add);
        field(
            t!("client certificates").into_owned(),
            control,
            Some(
                t!("PEM files sent when a server asks for one (mTLS) · a host uses its own row, else the one without a host · this machine only.")
                    .into_owned(),
            ),
            cx,
        )
    }
}
