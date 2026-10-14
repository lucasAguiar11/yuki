//! Voleeo bundle (`yuki_core::VoleeoBundle`, YAML or JSON) → `ConvertedPlan`.
//! Core types map 1:1, so this skips the IR: ids are regenerated, parents
//! remapped, and workspace/folder auth + headers flattened onto requests like
//! every other format. Several bundled workspaces land as one top-level folder
//! each. Preview/selection reuse the `preview_nodes` positional ids over folders
//! and HTTP requests (ws/gRPC items aren't preview nodes; see `plan`).

use crate::convert::{
    merge_headers, new_folder, request_auth, scope_auth, ConvertedPlan, ImportNode,
};
use crate::{ImportError, ImportFormat, ImportPreview};
use std::collections::{HashMap, HashSet};
use yuki_core::{
    new_id, ApiFolder, AuthConfig, EnvironmentKind, HttpRequest, InheritSource, RequestParameter,
    VoleeoBundle, VoleeoWorkspace, VOLEEO_BUNDLE_VERSION,
};

/// Parse and mark every auth secret plaintext: a bundle file is plaintext, and
/// a stray `*_encrypted: true` (hand-edited bundle) makes an unencrypted target
/// workspace refuse the request (`workspace_encryption_required`). Variable
/// `encrypted` flags stay: they mark secret variables, which an encrypted
/// target seals on save and an unencrypted one reads as plaintext.
fn parse(content: &str) -> Result<VoleeoBundle, ImportError> {
    let mut b: VoleeoBundle = serde_json::from_str(content)
        .or_else(|_| serde_yaml::from_str(content))
        .map_err(|e| ImportError::Parse(e.to_string()))?;
    for w in &mut b.workspaces {
        let auths = std::iter::once(&mut w.workspace.auth)
            .chain(w.folders.iter_mut().map(|f| &mut f.auth))
            .chain(w.requests.iter_mut().map(|r| &mut r.auth))
            .chain(w.websockets.iter_mut().map(|c| &mut c.auth))
            .chain(w.grpc.iter_mut().map(|g| &mut g.auth));
        auths.for_each(AuthConfig::mark_secrets_plaintext);
    }
    Ok(b)
}

fn version_warning(b: &VoleeoBundle) -> Option<String> {
    (b.voleeo_bundle != VOLEEO_BUNDLE_VERSION).then(|| {
        format!(
            "Bundle version {} isn't {VOLEEO_BUNDLE_VERSION}; fields this version doesn't know were skipped.",
            b.voleeo_bundle
        )
    })
}

enum Child<'a> {
    Folder(&'a ApiFolder),
    Request(&'a HttpRequest),
}

/// Folders + HTTP requests directly under `parent`, by `order` (folders first on ties).
fn children<'a>(w: &'a VoleeoWorkspace, parent: Option<&str>) -> Vec<Child<'a>> {
    let folders = w
        .folders
        .iter()
        .filter(|f| f.folder_id.as_deref() == parent);
    let requests = w
        .requests
        .iter()
        .filter(|r| r.folder_id.as_deref() == parent);
    let mut kids: Vec<(f64, Child)> = folders
        .map(|f| (f.order, Child::Folder(f)))
        .chain(requests.map(|r| (r.order, Child::Request(r))))
        .collect();
    kids.sort_by(|a, b| a.0.total_cmp(&b.0));
    kids.into_iter().map(|(_, c)| c).collect()
}

/// Positional prefix of each workspace's root: `""` for a lone workspace, else
/// `"{i}."` under its group folder node `"{i}"`.
fn root_prefix(b: &VoleeoBundle, i: usize) -> String {
    if b.workspaces.len() == 1 {
        String::new()
    } else {
        format!("{i}.")
    }
}

pub fn preview(content: &str) -> Result<ImportPreview, ImportError> {
    let b = parse(content)?;
    let mut tree = Vec::new();
    for (i, w) in b.workspaces.iter().enumerate() {
        let kids = nodes(w, None, &root_prefix(&b, i));
        if b.workspaces.len() == 1 {
            tree = kids;
        } else {
            tree.push(folder_node(i.to_string(), &w.workspace.name, kids));
        }
    }
    let variable_count = b
        .workspaces
        .iter()
        .flat_map(|w| &w.environments)
        .map(|e| e.variables.len())
        .sum::<usize>() as u32;
    Ok(ImportPreview {
        format: ImportFormat::Voleeo,
        format_version: Some(b.voleeo_bundle.clone()),
        suggested_name: match b.workspaces.as_slice() {
            [w] => w.workspace.name.clone(),
            _ => "Voleeo bundle".into(),
        },
        tree,
        variable_count,
        warnings: version_warning(&b)
            .into_iter()
            .chain(b.workspaces.iter().filter_map(dns_warning))
            .collect(),
    })
}

fn nodes(w: &VoleeoWorkspace, parent: Option<&str>, prefix: &str) -> Vec<ImportNode> {
    children(w, parent)
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            let id = format!("{prefix}{i}");
            match c {
                Child::Folder(f) => {
                    let kids = nodes(w, Some(&f.id), &format!("{id}."));
                    folder_node(id, &f.name, kids)
                }
                Child::Request(r) => ImportNode {
                    id,
                    kind: "request".into(),
                    name: r.name.clone(),
                    method: Some(r.method.clone()),
                    path: Some(r.url.clone()),
                    description: None,
                    children: Vec::new(),
                },
            }
        })
        .collect()
}

fn folder_node(id: String, name: &str, children: Vec<ImportNode>) -> ImportNode {
    ImportNode {
        id,
        kind: "folder".into(),
        name: name.to_string(),
        method: None,
        path: None,
        description: None,
        children,
    }
}

fn dns_warning(w: &VoleeoWorkspace) -> Option<String> {
    let n = w.workspace.dns_overrides.len();
    (n > 0).then(|| {
        format!(
            "Workspace \"{}\": {n} DNS override(s) weren't imported — re-add them in the \
             workspace settings.",
            w.workspace.name
        )
    })
}

/// Selection keeps folders/requests like `filter_items`; a ws/gRPC item is kept
/// when its parent folder survives, or (at the workspace root) when anything of
/// that workspace survives.
pub fn plan(
    content: &str,
    selected: Option<&[String]>,
    workspace_id: &str,
    order_base: f64,
) -> Result<ConvertedPlan, ImportError> {
    let b = parse(content)?;
    let selected: Option<HashSet<&str>> =
        selected.map(|ids| ids.iter().map(String::as_str).collect());
    let mut walk = Walk {
        sel: selected.as_ref(),
        ws_id: workspace_id,
        order: order_base,
        out: ConvertedPlan::default(),
        kept: HashMap::new(),
    };
    for (i, w) in b.workspaces.iter().enumerate() {
        walk.workspace(w, i, b.workspaces.len() > 1);
    }
    walk.out.warnings.extend(version_warning(&b));
    Ok(walk.out)
}

struct Walk<'a> {
    sel: Option<&'a HashSet<&'a str>>,
    ws_id: &'a str,
    order: f64,
    out: ConvertedPlan,
    /// Current workspace: kept source folder id → (new id, auth in effect there).
    kept: HashMap<String, (String, AuthConfig)>,
}

/// `Inherit { from: Workspace }` skips folders straight to the workspace auth;
/// the default `Inherit` takes the enclosing scope's.
fn resolve(own: &AuthConfig, scope: &AuthConfig, w: &VoleeoWorkspace) -> AuthConfig {
    match own {
        AuthConfig::Inherit {
            from: InheritSource::Workspace,
        } => scope_auth(&w.workspace.auth, &AuthConfig::None),
        own => request_auth(own, scope),
    }
}

impl Walk<'_> {
    fn next_order(&mut self) -> f64 {
        self.order += 1.0;
        self.order - 1.0
    }

    fn selected(&self, id: &str) -> bool {
        self.sel.is_none_or(|s| s.contains(id))
    }

    fn workspace(&mut self, w: &VoleeoWorkspace, i: usize, grouped: bool) {
        let root_auth = scope_auth(&w.workspace.auth, &AuthConfig::None);
        let root = (&root_auth, w.workspace.headers.as_slice());
        // Root items' new parent; `None` when the root (group) was dropped.
        let root_parent: Option<Option<String>> = if grouped {
            let group = new_folder(self.ws_id, None, &w.workspace.name, self.next_order());
            let gid = group.id.clone();
            self.keep_folder(group, &i.to_string(), |walk| {
                walk.walk(w, None, Some(&gid), &format!("{i}."), root)
            })
            .then_some(Some(gid))
        } else {
            let before = self.out.folders.len() + self.out.requests.len();
            self.walk(w, None, None, "", root);
            let any = self.out.folders.len() + self.out.requests.len() > before;
            (self.sel.is_none() || any).then_some(None)
        };

        let kept = std::mem::take(&mut self.kept);
        let parent_of = |folder_id: &Option<String>| match folder_id {
            None => root_parent.clone().map(|p| (p, root_auth.clone())),
            Some(f) => kept.get(f).map(|(id, a)| (Some(id.clone()), a.clone())),
        };
        // Headers stay HTTP-only: folder/workspace headers aren't merged into
        // WS handshakes or gRPC metadata.
        for c in &w.websockets {
            if let Some((parent, scope)) = parent_of(&c.folder_id) {
                let mut c = c.clone();
                c.id = new_id();
                c.workspace_id = self.ws_id.to_string();
                c.folder_id = parent;
                c.auth = resolve(&c.auth, &scope, w);
                c.order = self.next_order();
                self.out.websockets.push(c);
            }
        }
        for g in &w.grpc {
            if let Some((parent, scope)) = parent_of(&g.folder_id) {
                let mut g = g.clone();
                g.id = new_id();
                g.workspace_id = self.ws_id.to_string();
                g.folder_id = parent;
                g.auth = resolve(&g.auth, &scope, w);
                g.order = self.next_order();
                self.out.grpc.push(g);
            }
        }

        for env in &w.environments {
            match env.kind {
                EnvironmentKind::Global => self.out.variables.extend(env.variables.clone()),
                EnvironmentKind::Personal => {
                    let mut env = env.clone();
                    env.id = new_id();
                    env.workspace_id = self.ws_id.to_string();
                    self.out.environments.push(env);
                }
            }
        }
        self.out.warnings.extend(dns_warning(w));
    }

    /// Push `folder`, fill it via `fill` (returns its preview child count), and
    /// drop it again unless something landed inside or it's a selected empty
    /// folder (`filter_items` rule). Returns whether it was kept.
    fn keep_folder(
        &mut self,
        folder: ApiFolder,
        pos_id: &str,
        fill: impl FnOnce(&mut Self) -> usize,
    ) -> bool {
        let idx = self.out.folders.len();
        let requests = self.out.requests.len();
        self.out.folders.push(folder);
        let n = fill(self);
        let landed = self.out.folders.len() > idx + 1 || self.out.requests.len() > requests;
        let keep = self.sel.is_none() || landed || (n == 0 && self.selected(pos_id));
        if !keep {
            self.out.folders.truncate(idx);
        }
        keep
    }

    /// Emit `parent`'s children under `new_parent`; `scope` = (auth, headers) in
    /// effect there. Returns how many preview children `parent` has.
    fn walk(
        &mut self,
        w: &VoleeoWorkspace,
        parent: Option<&str>,
        new_parent: Option<&str>,
        prefix: &str,
        scope: (&AuthConfig, &[RequestParameter]),
    ) -> usize {
        let kids = children(w, parent);
        for (i, c) in kids.iter().enumerate() {
            let id = format!("{prefix}{i}");
            match c {
                Child::Folder(f) => {
                    let mut nf = new_folder(self.ws_id, new_parent, &f.name, self.next_order());
                    nf.variables = f.variables.clone();
                    nf.color = f.color.clone();
                    let nid = nf.id.clone();
                    let auth = scope_auth(&f.auth, scope.0);
                    let headers = merge_headers(scope.1, &f.headers);
                    let keep = self.keep_folder(nf, &id, |walk| {
                        let p = format!("{id}.");
                        walk.walk(w, Some(&f.id), Some(&nid), &p, (&auth, &headers))
                    });
                    if keep {
                        self.kept.insert(f.id.clone(), (nid, auth));
                    }
                }
                Child::Request(r) if self.selected(&id) => {
                    let order = self.next_order();
                    self.out.requests.push(HttpRequest {
                        id: new_id(),
                        workspace_id: self.ws_id.to_string(),
                        folder_id: new_parent.map(str::to_string),
                        headers: merge_headers(scope.1, &r.headers),
                        auth: resolve(&r.auth, scope.0, w),
                        order,
                        ..(*r).clone()
                    });
                }
                Child::Request(_) => {}
            }
        }
        kids.len()
    }
}
