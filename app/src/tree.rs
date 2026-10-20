use std::collections::{HashMap, HashSet};

use gpui_kit::SharedString;
use yuki_core::{ApiFolder, GitChange, GitFileChange, HttpRequest};

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

/// Whether `folder_id` is `ancestor` or nested under it. Guards moves that
/// would make a folder its own parent; bounded like `folder_path`.
pub fn within<'a>(
    folders: &'a [ApiFolder],
    mut folder_id: Option<&'a str>,
    ancestor: &str,
) -> bool {
    for _ in 0..=folders.len() {
        match folder_id {
            Some(id) if id == ancestor => return true,
            Some(id) => {
                folder_id = folders
                    .iter()
                    .find(|f| f.id == id)
                    .and_then(|f| f.folder_id.as_deref())
            }
            None => return false,
        }
    }
    false
}

/// Sidebar git marks: each changed node, and every folder above it with each
/// kind of change inside (`M A D`), most pressing first (conflict, deleted,
/// modified, renamed, added) — a folder says what changed in it, collapsed
/// or not. Untracked counts as added: one letter per kind. A deleted node is
/// gone from the tree, so it climbs from the folder its HEAD file named.
pub fn marks(
    folders: &[ApiFolder],
    requests: &[HttpRequest],
    changed: &[GitFileChange],
) -> HashMap<String, Vec<GitChange>> {
    fn rank(c: GitChange) -> u8 {
        match c {
            GitChange::Conflicted => 0,
            GitChange::Deleted => 1,
            GitChange::Modified => 2,
            GitChange::Renamed => 3,
            GitChange::Added | GitChange::Untracked => 4,
        }
    }
    let parent_of = |id: &str| {
        requests
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.folder_id.as_deref())
            .or_else(|| {
                folders
                    .iter()
                    .find(|f| f.id == id)
                    .map(|f| f.folder_id.as_deref())
            })
            .flatten()
    };
    let mut out: HashMap<String, Vec<GitChange>> = HashMap::new();
    for c in changed {
        let Some(id) = c.node_id.as_deref() else {
            continue;
        };
        let change = match c.change {
            GitChange::Untracked => GitChange::Added,
            other => other,
        };
        let mut node = Some(id);
        let mut parent = parent_of(id).or(c.parent_id.as_deref());
        // Bounded by the folder count so a corrupt parent cycle can't hang the UI.
        for _ in 0..=folders.len() {
            let Some(n) = node else { break };
            let kinds = out.entry(n.to_string()).or_default();
            if !kinds.contains(&change) {
                kinds.push(change);
                kinds.sort_by_key(|&c| rank(c));
            }
            node = parent;
            parent = parent.and_then(parent_of);
        }
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(id: &str, parent: Option<&str>) -> ApiFolder {
        ApiFolder {
            id: id.into(),
            folder_type: "api".into(),
            model: "folder".into(),
            workspace_id: "ws".into(),
            folder_id: parent.map(Into::into),
            name: id.into(),
            headers: vec![],
            auth: yuki_core::AuthConfig::None,
            variables: vec![],
            color: None,
            order: 0.,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn within_follows_parents_and_stops_on_cycles() {
        let tree = [folder("a", None), folder("b", Some("a")), folder("c", None)];
        assert!(within(&tree, Some("a"), "a"));
        assert!(within(&tree, Some("b"), "a"));
        assert!(!within(&tree, Some("a"), "b"));
        assert!(!within(&tree, Some("c"), "a"));
        assert!(!within(&tree, None, "a"));
        let cycle = [folder("x", Some("y")), folder("y", Some("x"))];
        assert!(!within(&cycle, Some("x"), "z"));
    }

    #[test]
    fn marks_bubble_up_every_kind_once() {
        use GitChange::{Added, Deleted, Modified, Untracked};
        let change = |id: &str, change, parent: Option<&str>| GitFileChange {
            path: format!("req_{id}.yaml"),
            node_id: Some(id.into()),
            node_kind: yuki_core::GitNodeKind::Request,
            change,
            staged: false,
            parent_id: parent.map(Into::into),
        };
        let tree = [folder("a", None), folder("b", Some("a")), folder("c", None)];
        let req: HttpRequest = serde_json::from_value(serde_json::json!({
            "id": "r1", "type": "api", "model": "http_request", "workspaceId": "ws",
            "folderId": "b", "method": "GET", "name": "r1", "url": "",
            "createdAt": "", "updatedAt": ""
        }))
        .unwrap();
        let mut new_too = req.clone();
        new_too.id = "r3".into();
        let mut other = req.clone();
        other.id = "r2".into();
        other.folder_id = Some("a".into());
        let m = marks(
            &tree,
            &[req, new_too, other],
            &[
                change("r1", Added, None),
                change("r3", Untracked, None),
                change("r2", Modified, None),
                change("c", Added, None),
                // Gone from the tree: only git's HEAD parent places it.
                change("gone", Deleted, Some("b")),
            ],
        );
        assert_eq!(m["r1"], [Added]);
        assert_eq!(m["b"], [Deleted, Added]); // added + untracked: one `A`
        assert_eq!(m["a"], [Deleted, Modified, Added]); // every kind, worst first
        assert_eq!(m["c"], [Added]); // the folder's own change
        let cycle = [folder("x", Some("y")), folder("y", Some("x"))];
        assert_eq!(
            marks(&cycle, &[], &[change("x", Modified, Some("y"))]).len(),
            2
        );
    }
}
