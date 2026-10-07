//! Moving tree items: sidebar drag & drop and the "mover para…" palette.

use gpui_kit::component::command::CommandItem;
use gpui_kit::*;

use crate::{items::Op, palette::Target, tree, workspace::Yuki};

/// Payload of a sidebar row being dragged; also its drag preview.
#[derive(Clone)]
pub struct Dragged {
    pub id: SharedString,
    pub name: SharedString,
    /// Grab point inside the row; the preview sits past it, beside the
    /// cursor, so the row under the cursor stays readable.
    pub grab: Point<Pixels>,
}

impl Render for Dragged {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::component::ActiveTheme as _;
        let theme = cx.theme();
        div()
            .pl(self.grab.x + px(14.))
            .pt(self.grab.y + px(10.))
            .child(
                div()
                    .px_2()
                    .py_0p5()
                    .text_sm()
                    // Drawn outside the root view: inherits no text style.
                    .font_family(theme.font_family.clone())
                    .text_color(theme.popover_foreground)
                    .bg(theme.popover)
                    .border_1()
                    .border_color(theme.border)
                    .rounded(theme.radius)
                    .shadow_md()
                    .child(self.name.clone()),
            )
    }
}

/// Where a dropped row lands.
#[derive(Clone)]
pub enum Dest {
    /// Last child of this folder (`None` = root).
    Into(Option<String>),
    /// Right below this request, in its folder.
    After(SharedString),
}

impl Yuki {
    /// Folders `item` can move to, as `a / b` paths; never into itself.
    pub fn move_targets(&self, item: &SharedString) -> Vec<(CommandItem, Target)> {
        let Some(d) = &self.data else { return vec![] };
        let parent = d
            .folders
            .iter()
            .find(|f| f.id == **item)
            .map(|f| f.folder_id.clone())
            .or_else(|| self.request(item).map(|r| r.folder_id.clone()))
            .flatten();
        let target = |label: String, folder: Option<String>| {
            let row = CommandItem::new().label(label).checked(parent == folder);
            (
                row,
                Target::MoveTo {
                    item: item.clone(),
                    folder,
                },
            )
        };
        let mut folders: Vec<_> = d
            .folders
            .iter()
            .filter(|f| !tree::within(&d.folders, Some(&f.id), item))
            .map(|f| {
                (
                    tree::folder_path(&d.folders, Some(&f.id)).join(" / "),
                    f.id.clone(),
                )
            })
            .collect();
        folders.sort();
        std::iter::once(target("raiz".into(), None))
            .chain(folders.into_iter().map(|(path, id)| target(path, Some(id))))
            .collect()
    }

    pub fn move_item(
        &mut self,
        id: SharedString,
        dest: Dest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(data) = &self.data else { return };
        let (parent, after) = match &dest {
            Dest::Into(p) => (p.clone(), None),
            Dest::After(r) if *r == id => return,
            Dest::After(r) => match self.request(r) {
                Some(r) => (r.folder_id.clone(), Some(r.order)),
                None => return,
            },
        };
        let folder = self.is_folder(&id);
        if folder && tree::within(&data.folders, parent.as_deref(), &id) {
            self.status = "uma pasta não pode ir para dentro de si mesma".into();
            cx.notify();
            return;
        }
        if let Some(p) = &parent {
            self.collapsed.remove(p);
        }
        let op: Op = Box::new(move |s, ws| {
            let order = match after {
                Some(o) => s.order_after(ws, parent.as_deref(), o)?,
                // Creation stamps `order` with the clock, so "now" sorts last.
                None => std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0., |d| d.as_millis() as f64),
            };
            if folder {
                s.update_folder_position(ws, &id, parent, order)?;
            } else {
                s.update_request_position(ws, &id, parent, order)?;
            }
            Ok(None)
        });
        self.mutate(false, op, window, cx);
    }
}
