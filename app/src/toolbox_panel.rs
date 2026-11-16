//! Right panel (⌘⇧T): the toolbox. One text box shared by the text tools
//! (switching tool keeps what was pasted); every derived value copies on click.

use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    v_flex, ActiveTheme as _, IconName, Sizable as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{
    toolbox::{self, Out, Tool},
    ui,
    workspace::Yuki,
    ToggleToolbox,
};

pub struct Toolbox {
    tool: Tool,
    input: Entity<TextareaState>,
    length: Entity<InputState>,
    symbols: bool,
    /// The password or UUIDs on display; redrawn by ↻ or a setting change.
    generated: Vec<Out>,
    /// `toolbox::NAMESPACES` index for UUID v3/v5.
    ns: usize,
    _subs: [Subscription; 2],
}

impl Toolbox {
    fn new(window: &mut Window, cx: &mut Context<Yuki>) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 10)
                .placeholder(t!("paste or type here").into_owned())
        });
        let length = cx.new(|cx| InputState::new(window, cx).default_value("20"));
        let subs = [
            cx.subscribe(&input, |_, _, ev: &InputEvent, cx| {
                if matches!(ev, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&length, |this: &mut Yuki, _, ev: &InputEvent, cx| {
                if let (InputEvent::Change, Some(tb)) = (ev, this.toolbox.as_mut()) {
                    tb.regenerate(cx);
                    cx.notify();
                }
            }),
        ];
        Self {
            tool: Tool::Base64,
            input,
            length,
            symbols: true,
            generated: vec![],
            ns: 0,
            _subs: subs,
        }
    }

    fn regenerate(&mut self, cx: &App) {
        self.generated = match self.tool {
            Tool::Uuid => toolbox::uuids(),
            _ => {
                let len = self.length.read(cx).value().trim().parse().unwrap_or(20);
                let pass = toolbox::password(len.clamp(4, 256), self.symbols);
                vec![("password", Ok(pass))]
            }
        };
    }
}

pub(crate) fn on_actions(el: Div, cx: &mut Context<Yuki>) -> Div {
    el.on_action(cx.listener(|this, _: &ToggleToolbox, window, cx| {
        this.toolbox = match this.toolbox.take() {
            Some(_) => None,
            None => Some(Toolbox::new(window, cx)),
        };
        this.vars_panel = false;
        this.git_panel = false;
        cx.notify();
    }))
}

impl Yuki {
    fn pick_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        if let Some(tb) = self.toolbox.as_mut() {
            tb.tool = tool;
            if !tool.takes_text() {
                tb.regenerate(cx);
            }
            cx.notify();
        }
    }
}

pub fn render(yuki: &Yuki, cx: &mut Context<Yuki>) -> impl IntoElement {
    let Some(tb) = &yuki.toolbox else {
        return div();
    };
    let tool = tb.tool;
    let text = tb.input.read(cx).value().to_string();
    let outs: Vec<Out> = match tool {
        Tool::Password | Tool::Uuid => tb.generated.clone(),
        Tool::Time => toolbox::derive(tool, &text),
        _ if text.is_empty() => vec![],
        _ => toolbox::derive(tool, &text),
    };

    // Text tools on one row, generators on the next: a deliberate split
    // rather than wherever the panel width happens to wrap.
    let (text_tabs, gen_tabs): (Vec<_>, Vec<_>) =
        Tool::ALL.into_iter().partition(|t| t.takes_text());
    let tabs = [text_tabs, gen_tabs].map(|group| {
        h_flex()
            .flex_wrap()
            .gap_x_3()
            .children(group.into_iter().map(|t| {
                chip(("tb-tool", t as usize), t.label(), t == tool, cx)
                    .on_click(cx.listener(move |this, _, _, cx| this.pick_tool(t, cx)))
            }))
    });
    let again = Button::new("tb-again")
        .small()
        .ghost()
        .label(format!("↻ {}", t!("generate")))
        .on_click(cx.listener(|this, _, _, cx| {
            if let Some(tb) = this.toolbox.as_mut() {
                tb.regenerate(cx);
                cx.notify();
            }
        }));
    let input = || h_flex().child(div().w_full().child(Textarea::new(&tb.input).small()));
    let border = cx.theme().border;
    // Name-based UUIDs have inputs of their own (namespace + name), so they get
    // a section under the random ones instead of crowding the top controls.
    let by_name = (tool == Tool::Uuid).then(|| {
        let namespaces = toolbox::NAMESPACES
            .iter()
            .enumerate()
            .map(|(ix, (name, _))| {
                chip(("tb-ns", ix), name, ix == tb.ns, cx).on_click(cx.listener(
                    move |this, _, _, cx| {
                        if let Some(tb) = this.toolbox.as_mut() {
                            tb.ns = ix;
                            cx.notify();
                        }
                    },
                ))
            });
        let named = toolbox::named_uuids(tb.ns, &text).into_iter().enumerate();
        v_flex()
            .gap_2()
            .pt_3()
            .border_t_1()
            .border_color(border)
            .child(
                h_flex()
                    .gap_x_3()
                    .text_xs()
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(
                        div()
                            .text_color(ui::faint(cx))
                            .child(t!("by name").into_owned()),
                    )
                    .children(namespaces),
            )
            .child(
                div()
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(input()),
            )
            .children(named.map(|(ix, (label, value))| out_row(100 + ix, label, value, cx)))
    });
    let controls = match tool {
        Tool::Uuid => h_flex().justify_end().child(again),
        Tool::Password => h_flex()
            .gap_2()
            .text_xs()
            .child(t!("length").into_owned())
            .child(div().w(px(56.)).child(Input::new(&tb.length).small()))
            .child(
                Checkbox::new("tb-symbols")
                    .checked(tb.symbols)
                    .xsmall()
                    .label(t!("symbols").into_owned())
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        if let Some(tb) = this.toolbox.as_mut() {
                            tb.symbols = *on;
                            tb.regenerate(cx);
                            cx.notify();
                        }
                    })),
            )
            .child(div().flex_1())
            .child(again),
        _ => v_flex().child(input()),
    };
    let rows: Vec<AnyElement> = outs
        .into_iter()
        .enumerate()
        .map(|(ix, (label, value))| out_row(ix, label, value, cx))
        .collect();
    let qr = (tool == Tool::Qr && !text.is_empty()).then(|| match toolbox::qr(&text) {
        Ok((width, modules)) => qr_code(width, modules, cx).into_any_element(),
        Err(e) => div()
            .text_color(ui::faint(cx))
            .child(t!(e).into_owned())
            .into_any_element(),
    });

    let theme = cx.theme();
    let header = h_flex()
        .h(px(30.))
        .px_3()
        .text_xs()
        .font_family(theme.mono_font_family.clone())
        .text_color(theme.muted_foreground)
        .child(t!("toolbox").into_owned());
    v_flex()
        .size_full()
        .bg(theme.sidebar)
        .child(header)
        .child(
            v_flex()
                .px_3()
                .pb_2()
                .gap_1()
                .text_xs()
                .font_family(theme.mono_font_family.clone())
                .border_b_1()
                .border_color(theme.border)
                .children(tabs),
        )
        .child(
            div()
                .p_3()
                .font_family(theme.mono_font_family.clone())
                .child(controls),
        )
        .child(
            v_flex()
                .id("tb-outs")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_3()
                .pb_3()
                .gap_3()
                .children(qr)
                .children(rows)
                .children(by_name),
        )
}

/// A tool or namespace pick: the active one bright and underlined.
fn chip(
    id: impl Into<ElementId>,
    label: &'static str,
    active: bool,
    cx: &Context<Yuki>,
) -> Stateful<Div> {
    let (fg, muted) = (cx.theme().foreground, cx.theme().muted_foreground);
    div()
        .id(id)
        .py_0p5()
        .cursor_pointer()
        .text_color(if active { fg } else { muted })
        .when(active, |d| d.border_b_1().border_color(fg))
        .hover(move |s| s.text_color(fg))
        .child(t!(label).into_owned())
}

/// `label` over the value; click copies it. A missing value says why, faint.
fn out_row(
    ix: usize,
    label: &'static str,
    value: Result<String, &'static str>,
    cx: &mut Context<Yuki>,
) -> AnyElement {
    let theme = cx.theme();
    let (fg, muted, hover) = (theme.foreground, theme.muted_foreground, theme.list_hover);
    let group = SharedString::from(format!("tb-out-{ix}"));
    let head = h_flex()
        .text_xs()
        .text_color(muted)
        .child(t!(label).into_owned());
    let body = div().text_xs().font_family(theme.mono_font_family.clone());
    let row = v_flex()
        .id(("tb-out", ix))
        .group(group.clone())
        .gap_0p5()
        .px_1()
        .py_0p5()
        .rounded_sm();
    match value {
        Err(why) => row
            .child(head)
            .child(body.text_color(ui::faint(cx)).child(t!(why).into_owned()))
            .into_any_element(),
        Ok(value) => row
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            // The copy mark shows on hover: it's what a click does.
            .child(
                head.child(div().flex_1()).child(
                    div()
                        .invisible()
                        .group_hover(group, |s| s.visible())
                        .child(IconName::Copy),
                ),
            )
            .child(body.text_color(fg).child(value.clone()))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(value.clone()));
                this.status = t!("%{label} copied", label = t!(label)).into_owned().into();
                cx.notify();
            }))
            .into_any_element(),
    }
}

/// Dark modules over a light square with the 4-module quiet zone scanners
/// need; whole-pixel modules, so edges stay sharp.
fn qr_code(width: usize, modules: Vec<bool>, cx: &App) -> impl IntoElement {
    // The app is dark-only: foreground is the light side, background the dark.
    let (light, dark) = (cx.theme().foreground, cx.theme().background);
    let side = width + 8;
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let m = (bounds.size.width / side as f32).floor().max(px(1.));
            window.paint_quad(fill(
                Bounds::new(bounds.origin, size(m * side as f32, m * side as f32)),
                light,
            ));
            for (i, _) in modules.iter().enumerate().filter(|(_, dark)| **dark) {
                let (x, y) = ((i % width + 4) as f32, (i / width + 4) as f32);
                let at = point(bounds.origin.x + m * x, bounds.origin.y + m * y);
                window.paint_quad(fill(Bounds::new(at, size(m, m)), dark));
            }
        },
    )
    .size(px(232.))
}
