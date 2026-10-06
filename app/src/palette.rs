use std::cell::Cell;

use gpui_kit::component::command::{Command, CommandItem, CommandState};
use gpui_kit::component::{ActiveTheme as _, IndexPath};
use gpui_kit::*;

use crate::workspace::Yuki;

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// ⌘P — jump to a request.
    Requests,
    /// ⌘⇧P — every app action.
    Commands,
    /// ⌘O — switch workspace.
    Workspaces,
}

/// What confirming row `i` does. Rows with an `Action` dispatch it themselves;
/// the rest are handled by `Yuki::palette_confirm`.
#[derive(Clone)]
pub enum Target {
    Action,
    Request(SharedString),
    Workspace(String),
}

/// Row pitch of the command list (text line + row padding).
const ROW_H: f32 = 28.;

pub struct Palette {
    pub kind: Kind,
    pub state: Entity<CommandState>,
    pub items: Vec<CommandItem>,
    pub targets: Vec<Target>,
    _rows: Subscription,
}

impl Palette {
    pub fn new(
        kind: Kind,
        items: Vec<(CommandItem, Target)>,
        window: &mut Window,
        cx: &mut Context<Yuki>,
    ) -> Self {
        let state = cx.new(|cx| CommandState::new(window, cx));
        state.update(cx, |s, cx| s.focus(window, cx));
        // The list height follows the match count, which only the state knows
        // and which changes as you type. Re-render only when the count moves:
        // the state notifies on every host render, so an unconditional notify
        // would loop.
        let seen = Cell::new(usize::MAX);
        let _rows = cx.observe(&state, move |_, state, cx| {
            let n = state.read(cx).matched_count();
            if seen.replace(n) != n {
                cx.notify();
            }
        });
        let (items, targets) = items.into_iter().unzip();
        Self {
            kind,
            state,
            items,
            targets,
            _rows,
        }
    }

    pub fn render(&self, cx: &mut Context<Yuki>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        let on_confirm = this.clone();
        let placeholder = match self.kind {
            Kind::Requests => "Ir para request…",
            Kind::Commands => "Executar comando…",
            Kind::Workspaces => "Trocar de workspace…",
        };
        let theme = cx.theme();
        // Backdrop: a click outside dismisses, like Zed's modals.
        div()
            .id("palette-backdrop")
            .absolute()
            .size_full()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(72.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close_palette(window, cx)),
            )
            .child(
                div()
                    .id("palette")
                    .w(px(560.))
                    // Height follows the list (capped below); a fixed height left
                    // a dead band under short lists.
                    .h_auto()
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .rounded(theme.radius_lg)
                    .shadow_lg()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        Command::new(&self.state)
                            .items(self.items.iter().cloned())
                            .placeholder(placeholder)
                            .bordered(false)
                            .max_h(px(list_height(self.state.read(cx).matched_count())))
                            .on_confirm(move |ix: IndexPath, window, cx| {
                                _ = on_confirm.update(cx, |this, cx| {
                                    this.palette_confirm(ix.row, window, cx)
                                });
                            })
                            .on_cancel(move |window, cx| {
                                _ = this.update(cx, |this, cx| this.close_palette(window, cx));
                            }),
                    ),
            )
    }
}

/// Exactly the visible rows (plus list padding), capped; room for the
/// "no results" line when nothing matches.
fn list_height(rows: usize) -> f32 {
    if rows == 0 {
        44.
    } else {
        (rows as f32 * ROW_H + 8.).min(420.)
    }
}
