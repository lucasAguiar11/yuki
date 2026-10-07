//! Inline rename of a sidebar row: ↵ or blur saves, esc cancels.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use crate::{items::Op, workspace::Yuki};

pub struct Rename {
    pub id: SharedString,
    pub input: Entity<InputState>,
    _sub: Subscription,
}

impl Yuki {
    pub fn start_rename(&mut self, id: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.name_of(&id) else {
            return;
        };
        let input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_value(name, window, cx);
            s.select_all(window, cx);
            s
        });
        let focus = input.read(cx).focus_handle(cx);
        focus.focus(window, cx);
        let _sub = cx.subscribe_in(&input, window, |this, _, ev: &InputEvent, window, cx| {
            if matches!(ev, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                this.commit_rename(window, cx);
            }
        });
        self.renaming = Some(Rename { id, input, _sub });
        cx.notify();
    }

    pub fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.renaming.take().is_some() {
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.renaming.take() else {
            return;
        };
        let name = r.input.read(cx).value().trim().to_string();
        self.focus_active(window, cx);
        cx.notify();
        if name.is_empty() || self.name_of(&r.id).as_deref() == Some(name.as_str()) {
            return;
        }
        let (folder, id) = (self.is_folder(&r.id), r.id.to_string());
        let op: Op = Box::new(move |s, ws| {
            if folder {
                s.rename_folder(ws, &id, name)?;
            } else {
                s.rename_request(ws, &id, name)?;
            }
            Ok(None)
        });
        self.mutate(false, op, window, cx);
    }
}
