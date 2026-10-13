use std::cell::Cell;

use gpui_kit::component::command::{Command, CommandItem, CommandState};
use gpui_kit::component::{ActiveTheme as _, Icon, IndexPath};
use gpui_kit::*;

use crate::{moving::Dest, workspace::Yuki, *};

#[derive(Clone, PartialEq)]
pub enum Kind {
    /// ⌘P — jump to a request.
    Requests,
    /// ⌘⇧P — every app action.
    Commands,
    /// ⌘O — switch workspace.
    Workspaces,
    /// Pick the destination folder of this item.
    Move(SharedString),
}

/// What confirming row `i` does. Rows with an `Action` dispatch it themselves;
/// the rest are handled by `Yuki::palette_confirm`.
#[derive(Clone)]
pub enum Target {
    Action,
    Request(SharedString),
    Workspace(String),
    MoveTo {
        item: SharedString,
        folder: Option<String>,
    },
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
            Kind::Requests => t!("Go to request…"),
            Kind::Commands => t!("Run command…"),
            Kind::Workspaces => t!("Switch workspace…"),
            Kind::Move(_) => t!("Move to…"),
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
                            .placeholder(placeholder.into_owned())
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

impl Yuki {
    pub(crate) fn open_palette(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        let items: Vec<(CommandItem, Target)> = match &kind {
            Kind::Requests => self
                .rows
                .iter()
                .filter(|r| r.method.is_some())
                .map(|r| {
                    let label = format!("{}  {}", r.method.as_deref().unwrap_or(""), r.name);
                    (
                        CommandItem::new().label(label),
                        Target::Request(r.id.clone()),
                    )
                })
                .collect(),
            Kind::Commands => {
                let action = |label: &str, a: Box<dyn Action>| {
                    (
                        CommandItem::new().label(label.to_string()).action(a),
                        Target::Action,
                    )
                };
                vec![
                    action(&t!("Send request"), Box::new(Send)),
                    action(&t!("Save"), Box::new(Save)),
                    action(&t!("New request"), Box::new(NewRequest)),
                    action(&t!("New folder"), Box::new(NewFolder)),
                    action(&t!("Rename request"), Box::new(RenameItem)),
                    action(&t!("Duplicate request"), Box::new(DuplicateItem)),
                    action(&t!("Copy as cURL"), Box::new(CopyAsCurl)),
                    action(&t!("Move request to…"), Box::new(MoveItem)),
                    action(&t!("Delete request"), Box::new(DeleteItem)),
                    action(&t!("Close tab"), Box::new(CloseTab)),
                    action(&t!("Toggle sidebar"), Box::new(ToggleSidebar)),
                    action("Git: commit, branches, pull/push…", Box::new(ToggleGit)),
                    action(&t!("Environments and variables…"), Box::new(OpenEnvs)),
                    action(&t!("Settings…"), Box::new(OpenSettings)),
                    action(&t!("Check for updates…"), Box::new(CheckUpdates)),
                    action(&t!("About Yuki"), Box::new(OpenAbout)),
                    action(&t!("Go to request…"), Box::new(OpenRequests)),
                    action(&t!("Switch workspace…"), Box::new(OpenWorkspaces)),
                    action(&t!("Report a bug…"), Box::new(ReportBug)),
                    action(&t!("Open folder as workspace…"), Box::new(OpenFolder)),
                    action(&t!("Import workspaces from Voleeo"), Box::new(ImportVoleeo)),
                ]
            }
            Kind::Workspaces => {
                let current = self.ws.as_ref().map(|w| w.id.as_str());
                let open = CommandItem::new()
                    .label(t!("+ open folder…").into_owned())
                    .icon(Icon::empty())
                    .action(Box::new(OpenFolder));
                self.workspaces
                    .iter()
                    .map(|w| {
                        // The paw marks where you are (same as the welcome
                        // list); others get an empty icon so labels line up.
                        let icon = if current == Some(w.id.as_str()) {
                            Icon::new(assets::IconName::PawPrint)
                        } else {
                            Icon::empty()
                        };
                        let item = CommandItem::new().label(w.name.clone()).icon(icon);
                        (item, Target::Workspace(w.id.clone()))
                    })
                    .chain([(open, Target::Action)])
                    .collect()
            }
            Kind::Move(item) => self.move_targets(item),
        };
        self.palette = Some(Palette::new(kind, items, window, cx));
        cx.notify();
    }

    pub fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = None;
        self.focus_active(window, cx);
        cx.notify();
    }

    pub fn palette_confirm(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let target = self
            .palette
            .as_ref()
            .and_then(|p| p.targets.get(row).cloned());
        self.close_palette(window, cx);
        match target {
            Some(Target::Request(id)) => self.open_request(&id, window, cx),
            Some(Target::Workspace(id)) => {
                if let Some(ws) = self.workspaces.iter().find(|w| w.id == id).cloned() {
                    self.open_workspace(ws, window, cx);
                }
            }
            Some(Target::MoveTo { item, folder }) => {
                self.move_item(item, Dest::Into(folder), window, cx)
            }
            Some(Target::Action) | None => {}
        }
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
