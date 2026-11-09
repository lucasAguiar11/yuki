//! Import destination: the workspace picked on the import screen and its
//! folder list.

use gpui_kit::component::{searchable_list::SearchableVec, IndexPath};
use gpui_kit::*;
use yuki_core::Workspace;

use crate::{import_view::ImportScreen, tree};

impl ImportScreen {
    pub(crate) fn picked_ws(&self, cx: &App) -> Option<&Workspace> {
        let ix = self.ws.read(cx).selected_index(cx).map_or(0, |ix| ix.row);
        self.workspaces.get(ix)
    }

    /// The folder select follows the picked workspace: its root, then each
    /// folder by its path. Reset to the root right away, so an import never
    /// lands in a folder id of the previous workspace.
    pub(crate) fn load_folders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.folder_ids = vec![None];
        self.folder.update(cx, |s, cx| {
            s.set_items(
                SearchableVec::new(vec![SharedString::from("/")]),
                window,
                cx,
            );
            s.set_selected_index(Some(IndexPath::new(0)), window, cx);
        });
        let Some(id) = self.picked_ws(cx).map(|w| w.id.clone()) else {
            return;
        };
        let backend = self.backend.clone();
        let ws_id = id.clone();
        let load = cx.background_spawn(async move { backend.api.requests.list_folders(&ws_id) });
        cx.spawn_in(window, async move |this, cx| {
            let folders = load.await.unwrap_or_default();
            _ = this.update_in(cx, |this, window, cx| {
                // Picked another workspace meanwhile: its own load wins.
                if this.picked_ws(cx).is_none_or(|w| w.id != id) {
                    return;
                }
                let (mut labels, mut ids) = (vec![SharedString::from("/")], vec![None]);
                for f in &folders {
                    let path = tree::folder_path(&folders, Some(&f.id)).join("/");
                    labels.push(format!("/{path}").into());
                    ids.push(Some(f.id.clone()));
                }
                this.folder_ids = ids;
                this.folder.update(cx, |s, cx| {
                    s.set_items(SearchableVec::new(labels), window, cx);
                    s.set_selected_index(Some(IndexPath::new(0)), window, cx);
                });
                cx.notify();
            });
        })
        .detach();
    }
}
