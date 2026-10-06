use std::collections::HashSet;

use gpui_kit::SharedString;
use yuki_core::{ApiFolder, HttpRequest};

#[derive(Clone)]
pub struct Row {
    pub id: SharedString,
    pub depth: usize,
    pub name: SharedString,
    /// `None` = folder.
    pub method: Option<SharedString>,
    pub open: bool,
}

/// Depth-first rows (folders before requests, then `order`, then name),
/// skipping children of collapsed folders.
pub fn rows(
    folders: &[ApiFolder],
    requests: &[HttpRequest],
    collapsed: &HashSet<String>,
) -> Vec<Row> {
    let mut out = Vec::new();
    walk(None, 0, folders, requests, collapsed, &mut out);
    out
}

/// Folder names from the root down to `folder_id`.
pub fn folder_path<'a>(folders: &'a [ApiFolder], mut folder_id: Option<&'a str>) -> Vec<String> {
    let mut path = Vec::new();
    // Bounded by the folder count so a corrupt parent cycle can't hang the UI.
    while let Some(id) = folder_id.filter(|_| path.len() < folders.len()) {
        let Some(f) = folders.iter().find(|f| f.id == id) else {
            break;
        };
        path.push(f.name.clone());
        folder_id = f.folder_id.as_deref();
    }
    path.reverse();
    path
}

fn walk(
    parent: Option<&str>,
    depth: usize,
    folders: &[ApiFolder],
    requests: &[HttpRequest],
    collapsed: &HashSet<String>,
    out: &mut Vec<Row>,
) {
    let mut subs: Vec<_> = folders
        .iter()
        .filter(|f| f.folder_id.as_deref() == parent)
        .collect();
    subs.sort_by(|a, b| {
        a.order
            .total_cmp(&b.order)
            .then_with(|| a.name.cmp(&b.name))
    });
    for f in subs {
        let open = !collapsed.contains(&f.id);
        out.push(Row {
            id: f.id.clone().into(),
            depth,
            name: f.name.clone().into(),
            method: None,
            open,
        });
        if open {
            walk(Some(&f.id), depth + 1, folders, requests, collapsed, out);
        }
    }
    let mut reqs: Vec<_> = requests
        .iter()
        .filter(|r| r.folder_id.as_deref() == parent)
        .collect();
    reqs.sort_by(|a, b| {
        a.order
            .total_cmp(&b.order)
            .then_with(|| a.name.cmp(&b.name))
    });
    out.extend(reqs.into_iter().map(|r| Row {
        id: r.id.clone().into(),
        depth,
        name: r.name.clone().into(),
        method: Some(r.method.clone().into()),
        open: false,
    }));
}
