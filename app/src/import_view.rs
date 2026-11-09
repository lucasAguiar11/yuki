//! "Import" screen state: the file's tree with checkboxes, where it lands (a
//! new workspace, or any existing one and a folder in it — the folder list in
//! `import_dest.rs`), and the import itself. Render in `import_render.rs`.

use std::{collections::HashSet, sync::Arc};

use gpui_kit::component::{
    input::InputState,
    searchable_list::SearchableVec,
    select::{SelectEvent, SelectState},
    IndexPath,
};
use gpui_kit::*;
use yuki_core::Workspace;
use yuki_import::{ImportFormat, ImportNode, ImportPreview};

use crate::{
    backend::Backend,
    transfer::{Dest, TransferEvent},
};

type NameSelect = SelectState<SearchableVec<SharedString>>;

pub struct ImportScreen {
    pub(crate) backend: Arc<Backend>,
    pub(crate) format: ImportFormat,
    content: Arc<String>,
    pub(crate) preview: ImportPreview,
    /// Selected request ids (preview's positional ids).
    pub(crate) checked: HashSet<String>,
    pub(crate) workspaces: Vec<Workspace>,
    /// Into the workspace picked in `ws` rather than a new one.
    pub(crate) existing: bool,
    pub(crate) name: Entity<InputState>,
    pub(crate) ws: Entity<NameSelect>,
    pub(crate) folder: Entity<NameSelect>,
    /// Parallel to `folder`'s rows; always the folders of the picked workspace.
    pub(crate) folder_ids: Vec<Option<String>>,
    _ws_picked: Subscription,
    pub focus: FocusHandle,
    pub(crate) error: SharedString,
    pub(crate) busy: bool,
}

impl EventEmitter<TransferEvent> for ImportScreen {}

pub fn format_label(f: ImportFormat) -> &'static str {
    match f {
        ImportFormat::OpenApi => "openapi",
        ImportFormat::Swagger2 => "swagger 2",
        ImportFormat::Postman => "postman",
        ImportFormat::Insomnia => "insomnia",
        ImportFormat::Bruno => "bruno",
        ImportFormat::Yaak => "yaak",
        ImportFormat::Voleeo => "voleeo",
    }
}

pub(crate) fn requests(nodes: &[ImportNode], out: &mut Vec<String>) {
    for n in nodes {
        out.extend(ids(n));
    }
}

/// The request ids a node stands for: itself, or everything below a folder.
fn ids(node: &ImportNode) -> Vec<String> {
    if node.kind == "request" {
        return vec![node.id.clone()];
    }
    let mut out = vec![];
    requests(&node.children, &mut out);
    out
}

impl ImportScreen {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        backend: Arc<Backend>,
        format: ImportFormat,
        content: String,
        preview: ImportPreview,
        workspaces: Vec<Workspace>,
        current: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut all = vec![];
        requests(&preview.tree, &mut all);
        let name =
            cx.new(|cx| InputState::new(window, cx).default_value(preview.suggested_name.clone()));
        let at = current
            .and_then(|id| workspaces.iter().position(|w| w.id == id))
            .unwrap_or(0);
        let names: Vec<SharedString> = workspaces.iter().map(|w| w.name.clone().into()).collect();
        // Not searchable: a filtered list would make `selected_index` point
        // into the matches, not into `workspaces`.
        let ws = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(names),
                Some(IndexPath::new(at)),
                window,
                cx,
            )
        });
        let folder = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(vec![SharedString::from("/")]),
                Some(IndexPath::new(0)),
                window,
                cx,
            )
            .searchable(true)
        });
        let _ws_picked =
            cx.subscribe_in(&ws, window, |this, _, ev: &SelectEvent<_>, window, cx| {
                if let SelectEvent::Confirm(Some(_)) = ev {
                    this.load_folders(window, cx);
                }
            });
        let mut this = Self {
            backend,
            format,
            content: Arc::new(content),
            preview,
            checked: all.into_iter().collect(),
            workspaces,
            existing: false,
            name,
            ws,
            folder,
            folder_ids: vec![None],
            _ws_picked,
            focus: cx.focus_handle(),
            error: "".into(),
            busy: false,
        };
        this.load_folders(window, cx);
        this
    }

    /// A folder checks or unchecks every request below it.
    pub(crate) fn toggle(&mut self, node: &ImportNode, on: bool, cx: &mut Context<Self>) {
        for id in ids(node) {
            if on {
                self.checked.insert(id);
            } else {
                self.checked.remove(&id);
            }
        }
        cx.notify();
    }

    pub(crate) fn all_checked(&self, node: &ImportNode) -> bool {
        let ids = ids(node);
        !ids.is_empty() && ids.iter().all(|id| self.checked.contains(id))
    }

    /// Something selected, or a variables-only file (Postman/Insomnia environment).
    pub(crate) fn can_import(&self) -> bool {
        !self.busy
            && (!self.checked.is_empty()
                || (self.preview.tree.is_empty() && self.preview.variable_count > 0))
    }

    pub(crate) fn confirm(&mut self, cx: &mut Context<Self>) {
        if !self.can_import() {
            return;
        }
        let dest = match self.picked_ws(cx).filter(|_| self.existing) {
            Some(ws) => {
                let ix = self
                    .folder
                    .read(cx)
                    .selected_index(cx)
                    .map_or(0, |ix| ix.row);
                Dest::Existing {
                    ws_id: ws.id.clone(),
                    folder: self.folder_ids.get(ix).cloned().flatten(),
                }
            }
            None => {
                let name = self.name.read(cx).value().trim().to_string();
                Dest::New(if name.is_empty() {
                    self.preview.suggested_name.clone()
                } else {
                    name
                })
            }
        };
        let selected: Vec<String> = self.checked.iter().cloned().collect();
        let (backend, format, content) = (self.backend.clone(), self.format, self.content.clone());
        self.busy = true;
        cx.notify();
        let task =
            cx.background_spawn(
                async move { backend.import(format, &content, Some(&selected), &dest) },
            );
        cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(done) => cx.emit(TransferEvent::Imported(Box::new(done))),
                    Err(e) => this.error = t!("import failed: %{e}", e = e).into_owned().into(),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
