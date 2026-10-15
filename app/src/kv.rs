//! Name/value rows with an on/off box (and a secret toggle for variables):
//! folder variables and folder/workspace headers. The last row is always
//! blank; typing in it adds the next one.

use gpui_kit::component::{
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex, ActiveTheme as _, Sizable as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_core::{new_id, EnvironmentVariable, RequestParameter};

use crate::ui;

/// A row was typed in, toggled or removed.
pub struct KvChanged;

struct Row {
    key: Entity<InputState>,
    value: Entity<InputState>,
    secret: bool,
    enabled: bool,
    _subs: [Subscription; 2],
}

pub struct KvRows {
    /// Variables have a secret column; headers don't.
    secrets: bool,
    rows: Vec<Row>,
}

impl EventEmitter<KvChanged> for KvRows {}

impl KvRows {
    pub fn variables(
        vars: &[EnvironmentVariable],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let rows = vars
            .iter()
            .map(|v| (v.key.as_str(), v.value.as_str(), v.encrypted, v.enabled));
        Self::new(true, rows, window, cx)
    }

    pub fn headers(
        headers: &[RequestParameter],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let rows = headers
            .iter()
            .map(|h| (h.name.as_str(), h.value.as_str(), false, h.enabled));
        Self::new(false, rows, window, cx)
    }

    fn new<'a>(
        secrets: bool,
        rows: impl Iterator<Item = (&'a str, &'a str, bool, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            secrets,
            rows: vec![],
        };
        for (k, v, secret, enabled) in rows {
            let row = this.row(k, v, secret, enabled, window, cx);
            this.rows.push(row);
        }
        let blank = this.row("", "", false, true, window, cx);
        this.rows.push(blank);
        this
    }

    fn row(
        &self,
        key: &str,
        value: &str,
        secret: bool,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Row {
        let mut input = |text: &str, placeholder: String, masked: bool, cx: &mut Context<Self>| {
            let text = text.to_string();
            cx.new(|cx| {
                let mut s = InputState::new(window, cx)
                    .placeholder(placeholder)
                    .masked(masked);
                s.set_value(text, window, cx);
                s
            })
        };
        let key = input(key, t!("name").into_owned(), false, cx);
        let value = input(value, t!("value").into_owned(), secret, cx);
        let on_change = |this: &mut Self,
                         input: &Entity<InputState>,
                         ev: &InputEvent,
                         window: &mut Window,
                         cx: &mut Context<Self>| {
            if matches!(ev, InputEvent::Change) {
                let last = this.rows.last();
                if last.is_some_and(|r| r.key == *input || r.value == *input) {
                    let blank = this.row("", "", false, true, window, cx);
                    this.rows.push(blank);
                }
                this.changed(cx);
            }
        };
        let _subs = [
            cx.subscribe_in(&key, window, on_change),
            cx.subscribe_in(&value, window, on_change),
        ];
        Row {
            key,
            value,
            secret,
            enabled,
            _subs,
        }
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(KvChanged);
        cx.notify();
    }

    /// Filled rows as `(name, value, secret, enabled)`.
    fn filled<'a>(
        &'a self,
        cx: &'a App,
    ) -> impl Iterator<Item = (String, String, bool, bool)> + 'a {
        self.rows
            .iter()
            .map(|r| {
                let (k, v) = (r.key.read(cx).value(), r.value.read(cx).value());
                (k.trim().to_string(), v.to_string(), r.secret, r.enabled)
            })
            .filter(|(k, v, ..)| !k.is_empty() || !v.is_empty())
    }

    pub fn to_variables(&self, cx: &App) -> Vec<EnvironmentVariable> {
        self.filled(cx)
            .map(|(key, value, encrypted, enabled)| EnvironmentVariable {
                key,
                value,
                encrypted,
                enabled,
            })
            .collect()
    }

    /// Ids kept by position: fresh ids would be a phantom diff in git.
    pub fn to_headers(&self, stored: &[RequestParameter], cx: &App) -> Vec<RequestParameter> {
        self.filled(cx)
            .enumerate()
            .map(|(ix, (name, value, _, enabled))| RequestParameter {
                id: stored.get(ix).map_or_else(new_id, |h| h.id.clone()),
                name,
                value,
                enabled,
            })
            .collect()
    }

    /// Filled rows, for the meta line count.
    pub fn count(&self, cx: &App) -> usize {
        self.filled(cx).count()
    }

    fn toggle_secret(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.rows.get_mut(ix) else {
            return;
        };
        r.secret = !r.secret;
        let masked = r.secret;
        r.value.update(cx, |s, cx| s.set_masked(masked, window, cx));
        self.changed(cx);
    }

    fn toggle_enabled(&mut self, ix: usize, on: bool, cx: &mut Context<Self>) {
        if let Some(r) = self.rows.get_mut(ix) {
            r.enabled = on;
            self.changed(cx);
        }
    }

    fn remove(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix + 1 < self.rows.len() {
            self.rows.remove(ix);
            self.changed(cx);
        }
    }
}

impl Render for KvRows {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (fg, faint, border, red, yellow, blue) = (
            theme.foreground,
            ui::faint(cx),
            theme.border,
            theme.red,
            theme.yellow,
            theme.blue,
        );
        let secrets = self.secrets;
        let head = h_flex()
            .h(px(26.))
            .gap_2()
            .text_xs()
            .text_color(faint)
            .border_b_1()
            .border_color(border)
            .child(div().w(px(16.)))
            .child(div().w(relative(0.3)).px_2p5().child(t!("name")))
            .child(div().flex_1().px_2p5().child(t!("value")))
            .when(secrets, |h| h.child(div().w(px(64.)).child(t!("secret"))))
            .child(div().w(px(16.)));
        let last = self.rows.len().saturating_sub(1);
        let rows = self.rows.iter().enumerate().map(|(ix, row)| {
            let blank = ix == last;
            let check = Checkbox::new(("kv-on", ix))
                .checked(row.enabled)
                .tab_stop(false)
                .xsmall()
                .on_click(
                    cx.listener(move |this, on: &bool, _, cx| this.toggle_enabled(ix, *on, cx)),
                );
            let secret = div()
                .id(("kv-secret", ix))
                .w(px(64.))
                .text_xs()
                .when(!blank, |d| {
                    d.cursor_pointer()
                        .text_color(if row.secret { yellow } else { faint })
                        .hover(move |s| s.text_color(fg))
                        .child(if row.secret {
                            t!("● yes")
                        } else {
                            t!("○ no")
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.toggle_secret(ix, window, cx)
                        }))
                });
            let remove = div()
                .id(("kv-rm", ix))
                .w(px(16.))
                .cursor_pointer()
                .text_color(faint)
                .opacity(0.)
                .when(!blank, |d| {
                    d.group_hover("kv-row", |s| s.opacity(1.))
                        .hover(move |s| s.text_color(red))
                        .child("×")
                        .on_click(cx.listener(move |this, _, _, cx| this.remove(ix, cx)))
                });
            let mut value = Input::new(&row.value).appearance(false);
            if row.secret {
                value = value.mask_toggle();
            }
            h_flex()
                .group("kv-row")
                .h(px(30.))
                .gap_2()
                .border_b_1()
                .border_color(border.opacity(0.5))
                .child(div().w(px(16.)).when(!blank, |d| d.child(check)))
                .child(
                    div()
                        .w(relative(0.3))
                        .text_color(blue)
                        .when(!row.enabled, |d| d.opacity(0.45))
                        .child(Input::new(&row.key).appearance(false)),
                )
                .child(
                    div()
                        .flex_1()
                        .text_color(fg)
                        .when(!row.enabled, |d| d.opacity(0.45))
                        .child(value),
                )
                .when(secrets, |h| h.child(secret))
                .child(remove)
        });
        v_flex().size_full().child(head).child(
            div()
                .id("kv-rows")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .children(rows.collect::<Vec<_>>()),
        )
    }
}
