//! Right panel (⌘⇧E): every variable the active request can see, grouped by
//! the layer that defines it, with the ones it uses marked and missing ones first.

use std::collections::BTreeMap;

use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex, ActiveTheme as _, IconName};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{
    var_view::MASK,
    vars::{Source, Var},
    workspace::Yuki,
};

pub fn render(yuki: &Yuki, cx: &mut Context<Yuki>) -> impl IntoElement {
    let theme = cx.theme();
    let header = h_flex()
        .h(px(30.))
        .px_3()
        .justify_between()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(t!("variables"))
        .child(
            h_flex()
                .id("reveal")
                .gap_1()
                .cursor_pointer()
                .hover(|s| s.text_color(theme.foreground))
                .child(if yuki.reveal {
                    IconName::EyeOff
                } else {
                    IconName::Eye
                })
                .child(if yuki.reveal {
                    t!("hide secrets")
                } else {
                    t!("reveal secrets")
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.reveal = !this.reveal;
                    cx.notify();
                })),
        );
    let shell = v_flex().size_full().bg(theme.sidebar).child(header);

    let Some(editor) = yuki.tabs.get(yuki.active).map(|e| e.read(cx)) else {
        return shell.child(
            div()
                .p_3()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(t!("open a request to see its variables")),
        );
    };
    let (used, missing, vars) = (
        editor.used_vars(cx),
        editor.missing_vars(cx),
        editor.vars.clone(),
    );
    let (red, muted, mono) = (
        theme.red,
        theme.muted_foreground,
        theme.mono_font_family.clone(),
    );
    let mut groups: BTreeMap<&Source, Vec<(&String, &Var)>> = BTreeMap::new();
    for (name, var) in vars.iter() {
        groups.entry(&var.source).or_default().push((name, var));
    }

    let mut list = v_flex().pb_3().text_xs().font_family(mono);
    if !missing.is_empty() {
        list = list
            .child(group_header(&t!("missing"), missing.len(), red, cx))
            .children(missing.iter().map(|name| {
                let key = name.clone();
                let fg = theme.foreground;
                row(name, t!("no value").into_owned(), true, red, muted).child(
                    div()
                        .id(SharedString::from(format!("create-{name}")))
                        .flex_none()
                        .cursor_pointer()
                        .text_color(muted)
                        .hover(move |s| s.text_color(fg))
                        .child(t!("+ create"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_envs(Some(key.clone()), window, cx)
                        })),
                )
            }));
    }
    for (source, vars) in groups {
        list = list.child(group_header(&source.label(), vars.len(), muted, cx));
        for (name, var) in vars {
            let shown = if var.secret && !yuki.reveal {
                MASK.to_string()
            } else {
                var.value.clone()
            };
            let is_used = used.contains(name);
            list = list.child(copy_row(name, shown, var.value.clone(), is_used, cx));
        }
    }
    shell.child(
        div()
            .id("vars-list")
            .flex_1()
            .overflow_y_scroll()
            .child(list),
    )
}

fn group_header(label: &str, count: usize, color: Hsla, cx: &App) -> impl IntoElement {
    h_flex()
        .mt_2()
        .px_3()
        .h(px(22.))
        .justify_between()
        .text_color(color)
        .child(label.to_string())
        .child(
            div()
                .text_color(cx.theme().muted_foreground.opacity(0.75))
                .child(count.to_string()),
        )
}

/// `● NAME   value  ⧉` — the dot marks variables the request uses; the copy
/// icon shows on hover (the caller wires the click).
fn row(
    name: &str,
    value: String,
    used: bool,
    name_color: Hsla,
    value_color: Hsla,
) -> Stateful<Div> {
    let group = SharedString::from(format!("var-{name}"));
    h_flex()
        .id(group.clone())
        .group(group.clone())
        .h(px(22.))
        .px_3()
        .gap_2()
        .child(
            div()
                .size(px(5.))
                .flex_none()
                .rounded_full()
                .when(used, |d| d.bg(name_color)),
        )
        .child(
            div()
                .w(px(120.))
                .flex_none()
                .truncate()
                .text_color(name_color)
                .child(name.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(value_color)
                .child(value),
        )
}

/// A defined variable: hover highlights, click copies the real value (even
/// when masked — copying is an explicit act).
fn copy_row(
    name: &str,
    shown: String,
    real: String,
    used: bool,
    cx: &mut Context<Yuki>,
) -> impl IntoElement {
    let theme = cx.theme();
    let name_color = if used {
        theme.foreground
    } else {
        theme.muted_foreground
    };
    let group = SharedString::from(format!("var-{name}"));
    let tip = SharedString::from(format!(
        "{name} = {shown}\n{}",
        t!("click to copy the value")
    ));
    let label = name.to_string();
    row(name, shown, used, name_color, theme.muted_foreground)
        .cursor_pointer()
        .hover(|s| s.bg(theme.list_hover))
        .child(
            div()
                .flex_none()
                .text_color(theme.muted_foreground)
                .invisible()
                .group_hover(group, |s| s.visible())
                .child(IconName::Copy),
        )
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(real.clone()));
            this.status = t!("%{label} copied", label = label).into_owned().into();
            cx.notify();
        }))
}
