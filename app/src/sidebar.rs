use gpui_kit::component::{
    h_flex,
    input::Input,
    menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem},
    tooltip::Tooltip,
    v_flex, ActiveTheme as _, Icon, IconName, Sizable as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{
    git_panel,
    moving::{Dest, Dragged},
    palette::Kind,
    tree, ui,
    workspace::Yuki,
    OpenWorkspaces,
};

const ROW_H: f32 = 24.;
const INDENT: f32 = 14.;

pub fn render(yuki: &Yuki, cx: &mut Context<Yuki>) -> impl IntoElement {
    let theme = cx.theme();
    let active_id = match yuki.scope.as_ref().filter(|_| yuki.scope_active) {
        Some((view, _)) => view.read(cx).folder.clone(),
        None => yuki
            .tabs
            .get(yuki.active)
            .map(|t| t.read(cx).saved.id.clone()),
    };
    let count = yuki.rows.iter().filter(|r| r.method.is_some()).count();
    let has_folders = yuki.data.as_ref().is_some_and(|d| !d.folders.is_empty());
    let any_open = yuki.any_folder_open();
    let this = cx.entity().downgrade();
    let (drop_bg, drop_line) = (theme.drop_target, theme.ring);
    let marks = match (&yuki.data, yuki.git.view.as_ref()) {
        (Some(d), Some(v)) => tree::marks(&d.folders, &d.requests, &v.changes),
        _ => Default::default(),
    };

    let rows = yuki.rows.iter().enumerate().map(|(ix, row)| {
        let selected = active_id.as_deref() == Some(row.id.as_ref());
        // Indent guides: one hairline per ancestor level.
        let guides = (0..row.depth).map(|_| {
            div()
                .w(px(INDENT))
                .h_full()
                .ml(px(5.))
                .border_l_1()
                .border_color(theme.border)
        });
        let lead: AnyElement = match &row.method {
            Some(m) => div()
                .w(px(36.))
                .flex_none()
                .text_xs()
                .font_family(theme.mono_font_family.clone())
                .text_color(ui::method_color(m, cx))
                .child(ui::method_short(m).to_string())
                .into_any_element(),
            None => {
                let key = row.id.to_string();
                div()
                    .id(("chevron", ix))
                    .w(px(16.))
                    .flex_none()
                    .text_color(theme.muted_foreground)
                    .hover(|s| s.text_color(theme.foreground))
                    .child(if row.open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        if !this.collapsed.remove(&key) {
                            this.collapsed.insert(key.clone());
                        }
                        this.refresh_rows();
                        cx.notify();
                    }))
                    .into_any_element()
            }
        };
        let id = row.id.clone();
        let is_folder = row.method.is_none();
        let renaming = yuki.renaming.as_ref().filter(|r| r.id == row.id);
        let label: AnyElement = match renaming {
            Some(r) => div()
                .flex_1()
                .key_context("Rename")
                .child(Input::new(&r.input).xsmall().text_sm())
                .into_any_element(),
            None => div()
                .flex_1()
                .truncate()
                .child(row.name.clone())
                .into_any_element(),
        };
        // `M`/`A` at the row end: changed since the last commit; a folder
        // shows each kind of change inside it (`M A`).
        let mark = marks.get(row.id.as_ref()).map(|kinds| {
            h_flex()
                .gap_1()
                .text_xs()
                .font_family(theme.mono_font_family.clone())
                .children(kinds.iter().map(|&c| {
                    let (letter, color) = git_panel::change_mark(c, cx);
                    div().text_color(color).child(letter)
                }))
        });
        let dest = if is_folder {
            Dest::Into(Some(id.to_string()))
        } else {
            Dest::After(id.clone())
        };
        let menu_this = this.clone();
        let menu_id = id.clone();
        h_flex()
            .id(ix)
            .h(px(ROW_H))
            // Fixed height even when the tree overflows (flex would squeeze rows).
            .flex_none()
            .pl_2()
            .pr_2()
            .text_sm()
            .cursor_pointer()
            .text_color(if is_folder {
                theme.muted_foreground
            } else {
                theme.foreground
            })
            .hover(|s| s.bg(theme.list_hover))
            // Always reserve the bar so selecting a row never shifts its text.
            .border_l_2()
            .border_color(if selected {
                theme.ring
            } else {
                transparent_black()
            })
            .when(selected, |s| s.bg(theme.list_active))
            .children(guides.collect::<Vec<_>>())
            .child(lead)
            .child(label)
            .children(mark)
            // The rename input takes the clicks while it is open.
            .when(renaming.is_none(), |s| {
                let dragged = Dragged {
                    id: id.clone(),
                    name: row.name.clone(),
                    grab: Point::default(),
                };
                s.on_drag(dragged, |d, grab, _, cx| {
                    cx.new(|_| Dragged { grab, ..d.clone() })
                })
                // Folders take the item in (tint); requests take it below
                // them (insertion line), so the cue matches where it lands.
                .drag_over::<Dragged>(move |s, _, _, _| {
                    if is_folder {
                        s.bg(drop_bg)
                    } else {
                        s.border_b_2().border_color(drop_line)
                    }
                })
                .on_drop(cx.listener(move |this, d: &Dragged, window, cx| {
                    this.move_item(d.id.clone(), dest.clone(), window, cx)
                }))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if is_folder {
                        // Opening shows what's inside too; only the chevron collapses.
                        if this.collapsed.remove(id.as_ref()) {
                            this.refresh_rows();
                        }
                        this.open_scope(Some(id.to_string()), None, window, cx);
                    } else {
                        this.open_request(&id, window, cx);
                    }
                }))
            })
            .context_menu(move |m, _, _| menu(m, &menu_this, Some(&menu_id), is_folder))
    });

    v_flex()
        .size_full()
        .bg(theme.sidebar)
        .child(
            h_flex()
                .mx_1p5()
                .mt_1()
                .gap_0p5()
                .child(
                    // Workspace switcher: the obvious place to look for "which project am I in".
                    h_flex()
                        .id("ws-switcher")
                        .flex_1()
                        .min_w_0()
                        .h(px(32.))
                        .px_1p5()
                        .gap_1p5()
                        .rounded(theme.radius)
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.list_hover))
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(
                                    yuki.ws
                                        .as_ref()
                                        .map_or("workspace".into(), |w| w.name.clone()),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(count.to_string()),
                        )
                        .child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(IconName::ChevronsUpDown),
                        )
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(OpenWorkspaces), cx)
                        }),
                )
                .when(active_id.is_some(), |s| {
                    s.child(
                        div()
                            .id("locate-request")
                            .p_1()
                            .rounded(theme.radius)
                            .cursor_pointer()
                            .text_color(theme.muted_foreground)
                            .hover(|s| s.text_color(theme.foreground).bg(theme.list_hover))
                            .child(Icon::new(assets::IconName::LocateFixed))
                            .tooltip(|window, cx| {
                                Tooltip::new(t!("locate current request").into_owned())
                                    .build(window, cx)
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.reveal_active(cx))),
                    )
                })
                .when(has_folders, |s| {
                    s.child(
                        div()
                            .id("toggle-folders")
                            .p_1()
                            .rounded(theme.radius)
                            .cursor_pointer()
                            .text_color(theme.muted_foreground)
                            .hover(|s| s.text_color(theme.foreground).bg(theme.list_hover))
                            .child(Icon::new(if any_open {
                                assets::IconName::ListChevronsDownUp
                            } else {
                                assets::IconName::ListChevronsUpDown
                            }))
                            .tooltip(move |window, cx| {
                                let tip = if any_open {
                                    t!("collapse all folders")
                                } else {
                                    t!("expand all folders")
                                };
                                Tooltip::new(tip.into_owned()).build(window, cx)
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_folders(cx))),
                    )
                }),
        )
        .child(
            v_flex()
                .id("tree")
                .flex_1()
                .track_scroll(&yuki.tree_scroll)
                .overflow_y_scroll()
                .children(rows.collect::<Vec<_>>())
                // Free space below the rows: drop here = move to the root;
                // right-click = create at the root.
                .child(
                    div()
                        .id("tree-end")
                        .flex_1()
                        .min_h(px(ROW_H * 2.))
                        .px_3()
                        .pt_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .when(yuki.rows.is_empty() && yuki.ws.is_some(), |s| {
                            s.child(t!("empty — ⌘N new request"))
                        })
                        .drag_over::<Dragged>(move |s, _, _, _| s.bg(drop_bg))
                        .on_drop(cx.listener(|this, d: &Dragged, window, cx| {
                            this.move_item(d.id.clone(), Dest::Into(None), window, cx)
                        }))
                        .context_menu(move |m, _, _| menu(m, &this, None, true)),
                ),
        )
        .when(yuki.workspaces.is_empty(), |s| {
            s.child(
                div()
                    .p_3()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(t!("no workspace — use :commands → Import")),
            )
        })
}

impl Yuki {
    /// Target button: open the active request's folders and scroll its row in.
    fn reveal_active(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let saved = &tab.read(cx).saved;
        let (id, mut folder) = (saved.id.clone(), saved.folder_id.clone());
        if let Some(d) = &self.data {
            // Bounded by the folder count so a parent cycle can't hang the UI.
            for _ in 0..d.folders.len() {
                let Some(f) = folder.take() else { break };
                folder = d
                    .folders
                    .iter()
                    .find(|x| x.id == f)
                    .and_then(|x| x.folder_id.clone());
                self.collapsed.remove(&f);
            }
        }
        self.refresh_rows();
        if let Some(ix) = self.rows.iter().position(|r| r.id.as_ref() == id) {
            self.tree_scroll.scroll_to_item(ix);
        }
        cx.notify();
    }

    fn any_folder_open(&self) -> bool {
        self.rows.iter().any(|r| r.method.is_none() && r.open)
    }

    /// Header button: collapse every folder if any is open, else expand them all.
    fn toggle_folders(&mut self, cx: &mut Context<Self>) {
        let Some(d) = &self.data else { return };
        if self.any_folder_open() {
            self.collapsed = d.folders.iter().map(|f| f.id.clone()).collect();
        } else {
            self.collapsed.clear();
        }
        self.refresh_rows();
        cx.notify();
    }
}

/// Right-click menu of a row (`id`) or of the empty area below the rows (`None`).
fn menu(
    mut m: PopupMenu,
    this: &WeakEntity<Yuki>,
    id: Option<&SharedString>,
    folder: bool,
) -> PopupMenu {
    if folder {
        let parent = id.map(|i| i.to_string());
        let p = parent.clone();
        m = m
            .item(item(this, "new request", move |y, w, cx| {
                y.new_item(false, p.clone(), w, cx)
            }))
            .item(item(this, "new folder", move |y, w, cx| {
                y.new_item(true, parent.clone(), w, cx)
            }))
            .when(id.is_none(), |m| {
                m.item(item(this, "workspace auth & headers…", |y, w, cx| {
                    y.open_scope(None, None, w, cx)
                }))
            });
    }
    let Some(id) = id.cloned() else { return m };
    if folder {
        m = m.separator();
    }
    let (a, b, c, d, e) = (id.clone(), id.clone(), id.clone(), id.clone(), id);
    m.item(item(this, "rename", move |y, w, cx| {
        y.start_rename(a.clone(), w, cx)
    }))
    .item(item(this, "duplicate", move |y, w, cx| {
        y.duplicate(b.clone(), w, cx)
    }))
    .item(item(this, "move to…", move |y, w, cx| {
        y.open_palette(Kind::Move(c.clone()), w, cx)
    }))
    .when(!folder, |m| {
        let f = e.clone();
        m.item(item(this, "copy as cURL", move |y, _, cx| {
            y.copy_as_curl(e.clone(), cx)
        }))
        .item(item(this, "copy for LLM", move |y, _, cx| {
            y.copy_for_llm(f.clone(), cx)
        }))
    })
    .separator()
    .item(item(this, "delete", move |y, w, cx| {
        y.confirm_delete(d.clone(), w, cx)
    }))
}

pub(crate) fn item(
    this: &WeakEntity<Yuki>,
    label: &'static str,
    f: impl Fn(&mut Yuki, &mut Window, &mut Context<Yuki>) + 'static,
) -> PopupMenuItem {
    let this = this.clone();
    PopupMenuItem::new(t!(label).to_string()).on_click(move |_, window, cx| {
        _ = this.update(cx, |y, cx| f(y, window, cx));
    })
}
