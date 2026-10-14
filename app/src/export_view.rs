//! "Export" screen: format, whether secrets go along, then a save dialog.

use std::{path::PathBuf, sync::Arc};

use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex, v_flex, ActiveTheme as _, Disableable as _, Sizable as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_core::Workspace;

use crate::{
    backend::Backend,
    settings_view::field,
    transfer::{self, ExportFormat, TransferEvent},
    CloseTransfer,
};

pub struct ExportScreen {
    backend: Arc<Backend>,
    ws: Workspace,
    format: ExportFormat,
    secrets: bool,
    pub focus: FocusHandle,
    error: SharedString,
    busy: bool,
}

impl EventEmitter<TransferEvent> for ExportScreen {}

impl ExportScreen {
    pub fn new(backend: Arc<Backend>, ws: Workspace, cx: &mut Context<Self>) -> Self {
        Self {
            backend,
            ws,
            format: ExportFormat::Postman,
            secrets: false,
            focus: cx.focus_handle(),
            error: "".into(),
            busy: false,
        }
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let dir = dirs::download_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        let path =
            cx.prompt_for_new_path(&dir, Some(&transfer::file_name(&self.ws.name, self.format)));
        let (backend, ws_id, format, secrets) = (
            self.backend.clone(),
            self.ws.id.clone(),
            self.format,
            self.secrets,
        );
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = path.await else {
                return;
            };
            _ = this.update(cx, |this, cx| {
                this.busy = true;
                cx.notify();
            });
            let task = cx.background_spawn(async move {
                let out = backend
                    .export(&ws_id, format, secrets)
                    .map_err(|e| e.to_string())?;
                transfer::write_export(&path, &out)?;
                Ok::<_, String>((path, out.warnings))
            });
            let result = task.await;
            _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok((path, warnings)) => cx.emit(TransferEvent::Exported(path, warnings)),
                    Err(e) => this.error = t!("export failed: %{e}", e = e).into_owned().into(),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for ExportScreen {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, muted, red, border, active_bg, hover_bg) = {
            let t = cx.theme();
            (
                t.foreground,
                t.muted_foreground,
                t.red,
                t.border,
                t.list_active,
                t.list_hover,
            )
        };
        let formats = ExportFormat::ALL.map(|f| {
            let on = f == self.format;
            div()
                .id(f.label())
                .px_2()
                .py_0p5()
                .rounded_sm()
                .cursor_pointer()
                .text_color(if on { fg } else { muted })
                .when(on, |s| s.bg(active_bg))
                .hover(move |s| s.bg(hover_bg))
                .child(f.label())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.format = f;
                    cx.notify();
                }))
        });
        let secrets = Checkbox::new("exp-secrets")
            .checked(self.secrets)
            .label(t!("include secrets").into_owned())
            .on_click(cx.listener(|this, on: &bool, _, cx| {
                this.secrets = *on;
                cx.notify();
            }));
        let secrets_hint = if self.secrets {
            t!("tokens, passwords and secret variables go in plain text.")
        } else {
            t!("secret values are written empty.")
        };
        v_flex()
            .size_full()
            .key_context("Transfer")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &CloseTransfer, _, cx| cx.emit(TransferEvent::Close)))
            .font_family(cx.theme().mono_font_family.clone())
            .text_sm()
            .child(
                h_flex()
                    .h(px(36.))
                    .px_4()
                    .border_b_1()
                    .border_color(border)
                    .child(
                        div()
                            .text_color(fg)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("{} · {}", t!("export"), self.ws.name)),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("exp-close")
                            .text_xs()
                            .cursor_pointer()
                            .text_color(muted)
                            .hover(move |s| s.text_color(fg))
                            .child(t!("close esc").into_owned())
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(CloseTransfer), cx)
                            }),
                    ),
            )
            .child(
                v_flex()
                    .px_4()
                    .py_3()
                    .gap_4()
                    .child(field(
                        t!("file format").into_owned(),
                        h_flex().gap_2().children(formats),
                        None,
                        cx,
                    ))
                    .child(field(
                        t!("secrets").into_owned(),
                        secrets,
                        Some(secrets_hint.into_owned()),
                        cx,
                    ))
                    .child(field(
                        "",
                        h_flex().child(
                            Button::new("exp-go")
                                .small()
                                .primary()
                                .label(
                                    if self.busy {
                                        t!("exporting…")
                                    } else {
                                        t!("export…")
                                    }
                                    .into_owned(),
                                )
                                .disabled(self.busy)
                                .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
                        ),
                        None,
                        cx,
                    ))
                    .when(!self.error.is_empty(), |s| {
                        s.child(div().text_xs().text_color(red).child(self.error.clone()))
                    }),
            )
    }
}
