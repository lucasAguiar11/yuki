//! Response field → variable, UI: a bar under a JSON response's body. It
//! follows the caret (`$.data.token = abc`), turns that field into a variable
//! (`→ variable`, ⌥↵) named in place (`{{ NAME }}`, ↵ / esc), and lists the
//! variables that read this request. The variable holds
//! `{{ $response.body(requestId="…", selector="$.path") }}`, so `{{ NAME }}`
//! always reads the latest response.

use gpui_kit::component::{
    h_flex,
    input::{Input, InputEvent, InputState},
    ActiveTheme as _, Sizable as _,
};
use gpui_kit::*;
use yuki_mcp::resolve::{is_identifier, select_json};

use crate::{
    capture::{formula, json_path_at, readers, suggest_name},
    editor::RequestEditor,
    ui,
};

/// The field picked from the response, waiting for a name.
pub struct Capture {
    path: String,
    name: Entity<InputState>,
    error: Option<String>,
    _sub: Subscription,
}

/// The JSON field under the response caret: its path and value.
pub struct CaretField {
    path: String,
    value: String,
}

/// Emitted when an environment variable changed here, so every tab reloads.
pub struct VarsChanged;

impl RequestEditor {
    /// Re-read the field under the caret when it moved (the response body
    /// notified). Parses the body once per caret move, not per frame.
    pub(crate) fn track_caret(&mut self, cx: &mut Context<Self>) {
        if !self.response_json {
            return;
        }
        let (text, cursor) = {
            let s = self.response_body.read(cx);
            (s.value(), s.cursor())
        };
        if self.caret_at == Some(cursor) {
            return;
        }
        self.caret_at = Some(cursor);
        // The root (caret at the top) isn't a field worth naming.
        self.caret = json_path_at(&text, cursor)
            .filter(|p| p != "$")
            .and_then(|path| {
                let value = select_json(&text, &path)?;
                Some(CaretField { path, value })
            });
        cx.notify();
    }

    /// `→ variable` / ⌥↵: name the field under the caret.
    pub(crate) fn start_capture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.caret.as_ref().map(|c| c.path.clone()) else {
            return;
        };
        let name = cx.new(|cx| InputState::new(window, cx).default_value(suggest_name(&path)));
        let handle = name.read(cx).focus_handle(cx);
        window.defer(cx, move |window, cx| handle.focus(window, cx));
        let _sub = cx.subscribe_in(&name, window, |this, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::PressEnter { .. }) {
                this.save_capture(cx);
            }
        });
        self.capture = Some(Capture {
            path,
            name,
            error: None,
            _sub,
        });
        cx.notify();
    }

    fn save_capture(&mut self, cx: &mut Context<Self>) {
        let Some(c) = self.capture.as_mut() else {
            return;
        };
        let name = c.name.read(cx).value().trim().to_string();
        if !is_identifier(&name) {
            c.error = Some(t!("letters, digits and _ only").into_owned());
            cx.notify();
            return;
        }
        let value = formula(&self.saved.id, &c.path);
        self.capture = None;
        self.write_var(name, Some(value), cx);
    }

    /// Set (or, with `None`, remove) a variable of the active environment.
    pub(crate) fn write_var(
        &mut self,
        name: String,
        value: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let backend = self.backend.clone();
        let (ws, env) = (self.saved.workspace_id.clone(), self.env_id.clone());
        let task = cx.background_spawn(async move {
            backend.set_var(&ws, env.as_deref(), &name, value.as_deref())
        });
        cx.spawn(async move |this, cx| {
            if let Err(e) = task.await {
                eprintln!("[capture] {e}");
            }
            _ = this.update(cx, |_, cx| cx.emit(VarsChanged));
        })
        .detach();
        cx.notify();
    }

    /// The bar under a JSON response's body (body tab only).
    pub(crate) fn render_capture_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.response_json || self.response.as_ref().is_none_or(|r| r.is_err()) {
            return None;
        }
        let found = readers(&self.vars, &self.saved.id);
        // The idle hint teaches the click; once any variable reads a response
        // it's learned, and the row only shows up for a caret or readers.
        if self.capture.is_none()
            && self.caret.is_none()
            && found.is_empty()
            && self
                .vars
                .iter()
                .any(|(_, v)| v.raw.contains("response.body("))
        {
            return None;
        }
        let theme = cx.theme();
        let (fg, muted, blue, green, red, accent) = (
            theme.foreground,
            theme.muted_foreground,
            theme.blue,
            theme.green,
            theme.red,
            theme.magenta,
        );
        let (chip_bg, border, mono) = (
            theme.secondary,
            theme.border,
            theme.mono_font_family.clone(),
        );
        let faint = ui::faint(cx);

        let left = match (&self.capture, &self.caret) {
            // Naming: `$.data.token → {{ NAME }}   ↵ save · esc cancel`
            (Some(c), _) => h_flex()
                .gap_2()
                .min_w_0()
                .child(div().text_color(blue).child(c.path.clone()))
                .child(div().text_color(faint).child("→"))
                .child(
                    h_flex()
                        .px_1()
                        .rounded(theme.radius)
                        .bg(chip_bg)
                        .child(div().text_color(faint).child("{{"))
                        .child(
                            div()
                                .w(px(180.))
                                .text_color(fg)
                                .child(Input::new(&c.name).appearance(false).xsmall()),
                        )
                        .child(div().text_color(faint).child("}}")),
                )
                .child(div().text_color(faint).child(t!("↵ save · esc cancel")))
                .children(c.error.clone().map(|e| div().text_color(red).child(e))),
            // Caret on a field: `$.data.token = abc   → variable ⌥↵`
            (None, Some(f)) => h_flex()
                .gap_2()
                .min_w_0()
                .child(div().flex_none().text_color(blue).child(f.path.clone()))
                .child(div().flex_none().text_color(faint).child("="))
                .child(
                    div()
                        .min_w_0()
                        .max_w(px(320.))
                        .truncate()
                        .text_color(green)
                        .child(f.value.clone()),
                )
                .child(
                    h_flex()
                        .id("to-var")
                        .flex_none()
                        .gap_1()
                        .ml_2()
                        .px_1p5()
                        .rounded(theme.radius)
                        .cursor_pointer()
                        .text_color(accent)
                        .hover(move |s| s.bg(chip_bg))
                        .child(t!("→ variable"))
                        .child(div().text_color(faint).child("⌥↵"))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.start_capture(window, cx)),
                        ),
                ),
            (None, None) => h_flex().child(
                div()
                    .text_color(faint)
                    .child(t!("click a JSON value to use it as a variable")),
            ),
        };

        // `ACCESS_TOKEN ← $.data.access_token ✕` for each variable reading this request.
        let readers = found.into_iter().map(|r| {
            let name = r.name.clone();
            h_flex()
                .id(SharedString::from(format!("reader-{}", r.name)))
                .flex_none()
                .gap_1()
                .child(div().text_color(blue).child(r.name.clone()))
                .child(div().text_color(faint).child(format!("← {}", r.selector)))
                .child(
                    div()
                        .id(SharedString::from(format!("unvar-{}", r.name)))
                        .px_0p5()
                        .cursor_pointer()
                        .text_color(faint)
                        .hover(move |s| s.text_color(red))
                        .child("✕")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.write_var(name.clone(), None, cx)
                        })),
                )
        });

        Some(
            h_flex()
                .id("capture-bar")
                .h(px(28.))
                .px_4()
                .gap_4()
                .flex_none()
                .text_xs()
                .font_family(mono)
                .text_color(muted)
                .border_t_1()
                .border_color(border)
                .capture_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                    if ev.keystroke.key == "escape" && this.capture.is_some() {
                        this.capture = None;
                        cx.stop_propagation();
                        cx.notify();
                    }
                }))
                .child(left.flex_1())
                .children(readers)
                .into_any_element(),
        )
    }
}
