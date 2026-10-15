//! Request variations in the editor: the `base ▾` chip leftmost in the meta
//! line (switch · new · rename · delete) and the save + rebuild every change
//! goes through. Each variation is an independent copy of the five sendable
//! fields; overlay in `yuki_core`, fold in `draft`.

use gpui_kit::component::{
    button::{Button, ButtonVariant, ButtonVariants as _},
    dialog::AlertDialog,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _,
};
use gpui_kit::*;
use yuki_core::{new_id, HttpRequest, Variation};

use crate::{
    editor::{EditorChanged, RequestEditor},
    CancelRename,
};

/// The name box for a new (`id: None`) or renamed variation.
pub struct Naming {
    id: Option<String>,
    input: Entity<InputState>,
    _sub: Subscription,
}

impl RequestEditor {
    pub(crate) fn render_variation(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(n) = &self.naming {
            return div()
                .key_context("Rename")
                .w(px(160.))
                .on_action(cx.listener(|this, _: &CancelRename, window, cx| {
                    this.naming = None;
                    this.url_focus(cx).focus(window, cx);
                    cx.notify();
                }))
                .child(Input::new(&n.input).xsmall())
                .into_any_element();
        }
        let req = self.draft(cx);
        let current = req.active().cloned();
        let label = match &current {
            Some(v) => format!("{} ▾", v.name),
            None => format!("{} ▾", t!("base")),
        };
        let items: Vec<(Option<String>, String)> = std::iter::once((None, t!("base").into_owned()))
            .chain(
                req.variations
                    .iter()
                    .map(|v| (Some(v.id.clone()), v.name.clone())),
            )
            .collect();
        let this = cx.entity().downgrade();
        let color = if current.is_some() {
            cx.theme().blue
        } else {
            cx.theme().muted_foreground
        };
        // A send or write in flight finishes with `mark_saved` of the request it
        // started with; switching under it would fold the new form over the old
        // variation and corrupt the base.
        Button::new("variation")
            .ghost()
            .xsmall()
            .disabled(self.sending || self.saving)
            .text_color(color)
            .label(label)
            .dropdown_menu(move |m, _, _| menu(m, &this, &items, current.as_ref()))
            .into_any_element()
    }

    /// Writes `f` applied to the request as edited, then rebuilds the form
    /// from it: the shown fields depend on which variation is active.
    fn change(
        &mut self,
        f: impl FnOnce(&mut HttpRequest),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sending || self.saving {
            return;
        }
        let mut req = self.draft(cx);
        f(&mut req);
        let notes = self.notes_text(cx);
        let backend = self.backend.clone();
        self.autosave = None;
        self.saving = true;
        let task = cx.background_spawn({
            let req = req.clone();
            async move {
                backend.save(&req)?;
                backend.save_notes(&req.workspace_id, &req.id, &notes)
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            _ = this.update_in(cx, |this, window, cx| match result {
                Ok(()) => {
                    this.rebuild(req, window, cx);
                    this.url_focus(cx).focus(window, cx);
                }
                Err(e) => {
                    this.saving = false;
                    this.response =
                        Some(Err(t!("failed to save: %{error}", error = e).into_owned()));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// A fresh form for `req`, keeping which pane is open.
    pub(crate) fn rebuild(
        &mut self,
        req: HttpRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (tab, open, env) = (self.req_tab, self.editor_open, self.env_id.clone());
        *self = RequestEditor::new(self.backend.clone(), req, env, window, cx);
        self.req_tab = tab;
        self.editor_open = open;
        cx.emit(EditorChanged);
        cx.notify();
    }

    fn start_naming(&mut self, id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let name = id
            .as_deref()
            .and_then(|id| self.saved.variations.iter().find(|v| v.id == id))
            .map(|v| v.name.clone())
            .unwrap_or_default();
        let input = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder(t!("variation name").into_owned());
            s.set_value(name, window, cx);
            s.select_all(window, cx);
            s
        });
        let _sub = cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                this.commit_naming(window, cx);
            }
        });
        // Deferred: the menu closing would pull focus back out.
        let focus = input.read(cx).focus_handle(cx);
        window.defer(cx, move |window, cx| focus.focus(window, cx));
        self.naming = Some(Naming { id, input, _sub });
        cx.notify();
    }

    fn commit_naming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(n) = self.naming.take() else { return };
        let name = n.input.read(cx).value().trim().to_string();
        cx.notify();
        if name.is_empty() {
            self.url_focus(cx).focus(window, cx);
            return;
        }
        self.change(
            move |req| match n.id {
                Some(id) => {
                    if let Some(v) = req.variations.iter_mut().find(|v| v.id == id) {
                        v.name = name;
                    }
                }
                None => {
                    // A copy of what's on screen (base or the active variation).
                    let id = new_id();
                    let copy = Variation::copy_of(&req.shown(), id.clone(), name);
                    req.variations.push(copy);
                    req.active_variation = Some(id);
                }
            },
            window,
            cx,
        );
    }

    fn confirm_delete(&mut self, v: Variation, window: &mut Window, cx: &mut Context<Self>) {
        let title: SharedString = t!("delete the variation “%{name}”?", name = v.name)
            .into_owned()
            .into();
        let this = cx.entity().downgrade();
        let build = move |alert: AlertDialog, _: &mut Window, _: &mut App| {
            let (this, id) = (this.clone(), v.id.clone());
            alert
                .title(title.clone())
                .description(t!("its overrides will be deleted from disk.").into_owned())
                .show_cancel(true)
                .ok_text(t!("delete").into_owned())
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(t!("cancel").into_owned())
                .on_ok(move |_, window, cx| {
                    let id = id.clone();
                    _ = this.update(cx, |e, cx| {
                        e.change(
                            |req| {
                                req.variations.retain(|v| v.id != id);
                                if req.active_variation.as_deref() == Some(id.as_str()) {
                                    req.active_variation = None;
                                }
                            },
                            window,
                            cx,
                        )
                    });
                    true
                })
        };
        // Deferred: the menu click would pull focus back out.
        let focus = self.url_focus(cx);
        window.defer(cx, move |window, cx| {
            focus.focus(window, cx);
            window.open_alert_dialog(cx, build);
        });
    }
}

fn menu(
    mut m: PopupMenu,
    this: &WeakEntity<RequestEditor>,
    items: &[(Option<String>, String)],
    current: Option<&Variation>,
) -> PopupMenu {
    let current_id = current.map(|v| v.id.clone());
    for (id, name) in items {
        let id = id.clone();
        let checked = id == current_id;
        m = m.item(
            item(this, name.clone(), move |e, w, cx| {
                let id = id.clone();
                e.change(|req| req.active_variation = id, w, cx)
            })
            .checked(checked),
        );
    }
    m = m
        .separator()
        .item(item(this, t!("new variation…"), |e, w, cx| {
            e.start_naming(None, w, cx)
        }));
    let Some(v) = current.cloned() else { return m };
    let id = v.id.clone();
    m = m
        .item(item(this, t!("rename variation…"), move |e, w, cx| {
            e.start_naming(Some(id.clone()), w, cx)
        }))
        .item(item(this, t!("delete variation"), move |e, w, cx| {
            e.confirm_delete(v.clone(), w, cx)
        }));
    m
}

fn item(
    this: &WeakEntity<RequestEditor>,
    label: impl Into<SharedString>,
    f: impl Fn(&mut RequestEditor, &mut Window, &mut Context<RequestEditor>) + 'static,
) -> PopupMenuItem {
    let this = this.clone();
    PopupMenuItem::new(label).on_click(move |_, window, cx| {
        _ = this.update(cx, |e, cx| f(e, window, cx));
    })
}
