//! The `{{` autocomplete popup (`complete.rs`) and the keys that drive it.

use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{
    complete::{CompleteHost, ItemKind, Shown},
    ui, CompleteAccept, CompleteDismiss, CompleteNext, CompleteOpen, CompletePrev,
};

const MAX_ITEMS: usize = 8;

/// The popup's actions on the host's root; while it shows, `VarComplete` makes
/// ↑↓↵⇥esc win over the input's own.
pub fn keys<H: CompleteHost, E: InteractiveElement>(el: E, open: bool, cx: &mut Context<H>) -> E {
    let el = if open {
        el.key_context("VarComplete")
    } else {
        el
    };
    el.on_action(
        cx.listener(|this, _: &CompleteNext, window, cx| this.move_completion(1, window, cx)),
    )
    .on_action(
        cx.listener(|this, _: &CompletePrev, window, cx| this.move_completion(-1, window, cx)),
    )
    .on_action(
        cx.listener(|this, _: &CompleteAccept, window, cx| {
            this.accept_completion(None, window, cx)
        }),
    )
    .on_action(cx.listener(|this, _: &CompleteDismiss, _, cx| this.dismiss_completion(cx)))
    .on_action(cx.listener(|this, _: &CompleteOpen, window, cx| this.open_completion(window, cx)))
}

pub fn render<H: CompleteHost>(
    host: &H,
    window: &mut Window,
    cx: &mut Context<H>,
) -> Option<AnyElement> {
    let Shown {
        target,
        items,
        hint,
    } = host.suggestions(window, cx)?;
    let (selected, scroll) = host.completion().as_ref().map_or((0, None), |c| {
        (
            c.selected.min(items.len().saturating_sub(1)),
            Some(c.scroll.clone()),
        )
    });
    let at = target.anchor(window, cx);
    let theme = cx.theme();
    let (blue, magenta, fg, muted, sel, radius) = (
        theme.blue,
        theme.magenta,
        theme.foreground,
        theme.muted_foreground,
        theme.secondary,
        theme.radius,
    );
    let faint = ui::faint(cx);
    let rows = items.into_iter().enumerate().map(|(ix, item)| {
        // A function shows a muted `e.g. …`: a sample, not what will be sent.
        let value = match item.kind {
            ItemKind::Function => div()
                .flex()
                .gap_1()
                .overflow_hidden()
                .child(
                    div()
                        .flex_none()
                        .text_color(faint)
                        .child(t!("e.g.").into_owned()),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_ellipsis()
                        .italic()
                        .text_color(muted)
                        .child(item.value),
                ),
            ItemKind::Namespace => div()
                .text_color(muted)
                .italic()
                .text_ellipsis()
                .child(item.value),
            _ => div().text_color(fg).text_ellipsis().child(item.value),
        };
        let color = match item.kind {
            ItemKind::Var => blue,
            _ => magenta,
        };
        h_flex()
            .id(ElementId::Name(format!("var-opt-{}", item.expr).into()))
            .h(px(24.))
            .flex_none()
            .px_2()
            .gap_3()
            .rounded(radius)
            .cursor_pointer()
            .when(ix == selected, |d| d.bg(sel))
            .hover(|d| d.bg(sel))
            .child(
                div()
                    .flex_none()
                    .max_w(px(260.))
                    .text_ellipsis()
                    .text_color(color)
                    .child(item.label),
            )
            .child(div().flex_1().min_w_0().child(value))
            .child(div().flex_none().text_color(faint).child(item.origin))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.accept_completion(Some(ix), window, cx)
                }),
            )
    });
    let theme = cx.theme();
    // MAX_ITEMS rows of 24 px with 2 px gaps; the rest scrolls.
    let rows = v_flex()
        .id("var-complete-rows")
        .max_h(px(MAX_ITEMS as f32 * 26. - 2.))
        .gap_0p5()
        .overflow_y_scroll()
        .when_some(scroll, |d, s| d.track_scroll(&s))
        .children(rows);
    let list = v_flex()
        .w(px(480.))
        .p_1()
        .gap_0p5()
        .bg(theme.popover)
        .border_1()
        .border_color(theme.border)
        .rounded(theme.radius)
        .font_family(theme.mono_font_family.clone())
        .text_xs()
        .child(rows)
        .child(
            v_flex()
                .px_2()
                .pt_1()
                .gap_0p5()
                .text_color(faint)
                .child(div().text_color(muted).child(hint))
                .child(t!("↑↓ choose · ↵ insert · esc close")),
        );
    Some(
        deferred(
            anchored()
                .position(at)
                .snap_to_window_with_margin(px(8.))
                .child(list),
        )
        .with_priority(1)
        .into_any_element(),
    )
}
