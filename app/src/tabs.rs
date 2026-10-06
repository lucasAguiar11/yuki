use gpui_kit::component::{h_flex, ActiveTheme as _, IconName};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{ui, workspace::Yuki};

/// Open requests as a terse strip: `GET photos •`. The active tab merges into
/// the document; dirty shows `•`, which turns into × on hover.
pub fn render(yuki: &Yuki, cx: &mut Context<Yuki>) -> impl IntoElement {
    let theme = cx.theme();
    let tabs = yuki.tabs.iter().enumerate().map(|(ix, editor)| {
        let e = editor.read(cx);
        let active = ix == yuki.active;
        let group = SharedString::from(format!("tab-{ix}"));
        let close = div()
            .id(("close", ix))
            .size(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme.radius)
            .text_color(theme.muted_foreground)
            .hover(|s| s.bg(theme.secondary_hover).text_color(theme.foreground))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.close_tab(ix, window, cx)
            }));
        let close = if e.dirty {
            close
                .child(
                    div()
                        .group_hover(group.clone(), |s| s.invisible())
                        .child("•"),
                )
                .into_any_element()
        } else {
            close
                .when(!active, |s| {
                    s.invisible().group_hover(group.clone(), |s| s.visible())
                })
                .child(IconName::Close)
                .into_any_element()
        };
        h_flex()
            .id(ix)
            .group(group.clone())
            .h_full()
            .pl_3()
            .pr_1p5()
            .gap_2()
            .text_xs()
            .cursor_pointer()
            .border_r_1()
            .border_color(theme.border)
            .when(active, |s| s.bg(theme.tab_active))
            .when(!active, |s| s.border_b_1())
            .text_color(if active {
                theme.tab_active_foreground
            } else {
                theme.tab_foreground
            })
            .hover(|s| s.text_color(theme.foreground))
            .child(
                div()
                    .font_family(theme.mono_font_family.clone())
                    .text_color(ui::method_color(&e.saved.method, cx))
                    .child(ui::method_short(&e.saved.method).to_string()),
            )
            .child(e.saved.name.clone())
            .child(close)
            .on_click(cx.listener(move |this, _, window, cx| this.activate_tab(ix, window, cx)))
    });
    h_flex()
        .id("tabs")
        .h(px(30.))
        .flex_none()
        .bg(theme.tab_bar)
        .overflow_x_scroll()
        .children(tabs.collect::<Vec<_>>())
        // Filler carries the bottom rule the active tab omits.
        .child(
            div()
                .flex_1()
                .h_full()
                .border_b_1()
                .border_color(theme.border),
        )
}
