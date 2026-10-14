use gpui_kit::component::{h_flex, ActiveTheme as _, IconName};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{moving::Dragged, ui, workspace::Yuki};

/// Payload of a tab being dragged (its index). Distinct from sidebar
/// `Dragged` so a tab dropped on the tree doesn't move the request.
struct DraggedTab(usize);

/// Open requests as a terse strip: `GET photos •`. The active tab merges into
/// the document; dirty shows `•`, which turns into × on hover.
pub fn render(yuki: &Yuki, cx: &mut Context<Yuki>) -> impl IntoElement {
    let theme = cx.theme();
    let drop_line = theme.ring;
    let last = yuki.tabs.len().saturating_sub(1);
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
            .on_drag(DraggedTab(ix), {
                let name = e.saved.name.clone();
                move |_, grab, _, cx| {
                    let name = name.clone().into();
                    cx.new(|_| Dragged {
                        id: SharedString::default(),
                        name,
                        grab,
                    })
                }
            })
            // Insertion line on the side the tab lands.
            .drag_over::<DraggedTab>(move |s, d, _, _| {
                if d.0 > ix {
                    s.border_l_2().border_color(drop_line)
                } else if d.0 < ix {
                    s.border_r_2().border_color(drop_line)
                } else {
                    s
                }
            })
            .on_drop(cx.listener(move |this, d: &DraggedTab, _, cx| this.move_tab(d.0, ix, cx)))
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
                .id("tabs-end")
                .flex_1()
                .h_full()
                .border_b_1()
                .border_color(theme.border)
                .drag_over::<DraggedTab>(move |s, _, _, _| s.border_l_2().border_color(drop_line))
                .on_drop(
                    cx.listener(move |this, d: &DraggedTab, _, cx| this.move_tab(d.0, last, cx)),
                ),
        )
}

impl Yuki {
    /// Drag & drop: `from` takes `to`'s place; the active tab stays active.
    fn move_tab(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if from == to || from >= self.tabs.len() || to >= self.tabs.len() {
            return;
        }
        let active = self.tabs.get(self.active).cloned();
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = active
            .and_then(|a| self.tabs.iter().position(|t| *t == a))
            .unwrap_or(0);
        cx.notify();
    }
}
