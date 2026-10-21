//! Right panel (⌘⇧G): the workspace's git — changes + commit, branches,
//! remote sync, conflicts after a pull, history.

use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    v_flex, ActiveTheme as _, Disableable as _, Sizable as _,
};
use gpui_kit::{prelude::FluentBuilder as _, *};
use yuki_core::GitChange;

use crate::{
    git::{ago, GitView},
    ui,
    workspace::Yuki,
};

pub fn change_mark(change: GitChange, cx: &App) -> (&'static str, Hsla) {
    let t = cx.theme();
    match change {
        GitChange::Modified => ("M", t.yellow),
        GitChange::Added | GitChange::Untracked => ("A", t.green),
        GitChange::Deleted => ("D", t.red),
        GitChange::Renamed => ("R", t.blue),
        GitChange::Conflicted => ("!", t.red),
    }
}

pub(crate) fn section(title: String, cx: &App) -> Div {
    div()
        .pt_3()
        .pb_1()
        .px_3()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(title)
}

fn link(id: impl Into<ElementId>, label: &'static str, cx: &App) -> Stateful<Div> {
    let (muted, fg) = (cx.theme().muted_foreground, cx.theme().foreground);
    div()
        .id(id)
        .px_1()
        .cursor_pointer()
        .text_color(muted)
        .hover(move |s| s.text_color(fg))
        .child(t!(label).into_owned())
}

pub fn render(yuki: &Yuki, cx: &mut Context<Yuki>) -> impl IntoElement {
    let theme = cx.theme();
    let (bg, fg, muted, border, mono) = (
        theme.sidebar,
        theme.foreground,
        theme.muted_foreground,
        theme.border,
        theme.mono_font_family.clone(),
    );
    // `git  ⎇ main                 committing…`
    let branch = yuki
        .git
        .view
        .as_ref()
        .and_then(|v| v.info.branch.clone())
        .map(|b| div().text_color(fg).child(format!("⎇ {b}")));
    let header = h_flex()
        .h(px(30.))
        .px_3()
        .gap_2()
        .text_xs()
        .font_family(mono.clone())
        .text_color(muted)
        .child("git")
        .children(branch)
        .child(div().flex_1())
        .children(yuki.git.busy.map(|b| div().child(t!(b).into_owned())));
    let shell = v_flex().size_full().bg(bg).child(header);

    let (Some(view), Some(_)) = (&yuki.git.view, &yuki.git.inputs) else {
        return shell.child(section(t!("loading…").into_owned(), cx));
    };
    if !view.info.is_repo {
        return shell.child(
            v_flex()
                .p_3()
                .gap_2()
                .text_xs()
                .text_color(muted)
                .child(t!("this workspace isn't versioned yet."))
                .child(t!("each workspace becomes a git repository in its own folder; the requests' YAML files are what goes into the commits."))
                .child(
                    Button::new("git-init")
                        .small()
                        .primary()
                        .label(t!("init repository").into_owned())
                        .on_click(cx.listener(|this, _, window, cx| this.git_init(window, cx))),
                )
                .children(error(yuki, cx)),
        );
    }

    // Pinned above the scroll: what blocks (conflicts, author) and the one
    // action this panel exists for (commit). The lists scroll below.
    let mut top = v_flex()
        .pb_3()
        .text_xs()
        .font_family(mono.clone())
        .border_b_1()
        .border_color(border);
    if !view.conflicts.is_empty() {
        top = top.child(conflicts(yuki, view, cx));
    }
    if !view.info.has_author {
        top = top.child(author(yuki, cx));
    } else {
        top = top.child(composer(yuki, view, cx));
    }
    top = top.children(error(yuki, cx));

    let body = v_flex()
        .pb_3()
        .text_xs()
        .font_family(mono)
        .child(crate::git_changes::render(yuki, view, cx))
        .child(branches(yuki, view, cx))
        .child(remote(yuki, view, cx))
        .child(history(view, cx));
    shell.child(top).child(
        div()
            .id("git-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(body),
    )
}

/// Failures show where the action was taken, not at the top of the panel.
fn error(yuki: &Yuki, cx: &App) -> Option<Div> {
    (!yuki.git.error.is_empty()).then(|| {
        div()
            .px_3()
            .pt_2()
            .text_xs()
            .text_color(cx.theme().red)
            .child(yuki.git.error.clone())
    })
}

fn author(yuki: &Yuki, cx: &mut Context<Yuki>) -> impl IntoElement {
    let inputs = yuki.git.inputs.as_ref().expect("inputs exist with a view");
    v_flex()
        .child(section(t!("who signs the commits").into_owned(), cx))
        .child(
            v_flex()
                .px_3()
                .gap_1()
                .child(Input::new(&inputs.name).small())
                .child(Input::new(&inputs.email).small())
                .child(
                    Button::new("git-identity")
                        .small()
                        .primary()
                        .w_full()
                        .label(t!("save author").into_owned())
                        .on_click(
                            cx.listener(|this, _, window, cx| this.git_identity_button(window, cx)),
                        ),
                ),
        )
}

/// `293 changes · 280 new · 13 modified`, message, commit.
fn composer(yuki: &Yuki, view: &GitView, cx: &mut Context<Yuki>) -> impl IntoElement {
    let inputs = yuki.git.inputs.as_ref().expect("inputs exist with a view");
    let n = view.changes.len();
    if n == 0 {
        return v_flex().child(section(t!("all committed").into_owned(), cx));
    }
    let count = |kinds: &[GitChange]| {
        view.changes
            .iter()
            .filter(|c| kinds.contains(&c.change))
            .count()
    };
    let mut summary = vec![t!(plural(n, "1 change", "%{n} changes"), n = n).into_owned()];
    for (k, one, many) in [
        (
            count(&[GitChange::Added, GitChange::Untracked]),
            "1 new",
            "%{n} new",
        ),
        (
            count(&[GitChange::Modified, GitChange::Renamed]),
            "1 modified",
            "%{n} modified",
        ),
        (count(&[GitChange::Deleted]), "1 deleted", "%{n} deleted"),
    ] {
        if k > 0 && k < n {
            summary.push(t!(plural(k, one, many), n = k).into_owned());
        }
    }
    let first = view.log.is_empty();
    let label = if first {
        t!("initial commit")
    } else {
        "commit".into()
    };
    let blocked = yuki.git.busy.is_some() || !view.conflicts.is_empty();
    v_flex().child(section(summary.join(" · "), cx)).child(
        v_flex()
            .px_3()
            .gap_1p5()
            .child(Input::new(&inputs.message).small())
            .child(
                Button::new("git-commit")
                    .small()
                    .primary()
                    .w_full()
                    .label(format!("{label} ({n})"))
                    .disabled(!yuki.git.has_message || blocked)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.git_commit_button(window, cx)),
                    ),
            ),
    )
}

fn plural(n: usize, one: &'static str, many: &'static str) -> &'static str {
    if n == 1 {
        one
    } else {
        many
    }
}

fn branches(yuki: &Yuki, view: &GitView, cx: &mut Context<Yuki>) -> impl IntoElement {
    let (fg, muted, active_bg) = (
        cx.theme().foreground,
        cx.theme().muted_foreground,
        cx.theme().list_active,
    );
    let rows = view.branches.iter().map(|b| {
        let name = b.name.clone();
        h_flex()
            .id(SharedString::from(format!("branch-{}", b.name)))
            .px_3()
            .h(px(22.))
            .gap_2()
            .cursor_pointer()
            .text_color(if b.current { fg } else { muted })
            .when(b.current, |s| s.bg(active_bg))
            .hover(move |s| s.text_color(fg))
            .child(if b.current { "●" } else { " " })
            .child(b.name.clone())
            .when(!b.current, |s| {
                s.on_click(cx.listener(move |this, _, window, cx| {
                    this.git_checkout(name.clone(), window, cx)
                }))
            })
    });
    let inputs = yuki.git.inputs.as_ref().expect("inputs exist with a view");
    // Unborn HEAD: no branch exists yet to list, switch to or branch from.
    let unborn = view.branches.is_empty();
    v_flex()
        .child(section("branches".into(), cx))
        .children(rows.collect::<Vec<_>>())
        .when(unborn, |s| {
            s.child(
                div().px_3().text_color(ui::faint(cx)).child(
                    t!(
                        "%{branch} · created on the first commit",
                        branch = view.info.branch.as_deref().unwrap_or("main")
                    )
                    .into_owned(),
                ),
            )
        })
        .when(!unborn, |s| {
            s.child(
                div()
                    .px_3()
                    .pt_1()
                    .child(Input::new(&inputs.branch).small()),
            )
        })
}

fn remote(yuki: &Yuki, view: &GitView, cx: &mut Context<Yuki>) -> impl IntoElement {
    let info = &view.info;
    let muted = cx.theme().muted_foreground;
    let inputs = yuki.git.inputs.as_ref().expect("inputs exist with a view");
    let origin = info.remotes.iter().find(|r| r.name == "origin");
    let body = match origin {
        None => div()
            .px_3()
            .child(Input::new(&inputs.remote).small())
            .into_any_element(),
        Some(r) => v_flex()
            .px_3()
            .gap_1()
            .child(div().truncate().text_color(muted).child(r.url.clone()))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("git-pull")
                            .small()
                            .outline()
                            .label(format!("pull ↓{}", info.behind))
                            .on_click(cx.listener(|this, _, window, cx| this.git_pull(window, cx))),
                    )
                    .child(
                        Button::new("git-push")
                            .small()
                            .outline()
                            .label(format!("push ↑{}", info.ahead))
                            .on_click(cx.listener(|this, _, window, cx| this.git_push(window, cx))),
                    ),
            )
            .into_any_element(),
    };
    v_flex().child(section("remote".into(), cx)).child(body)
}

fn conflicts(yuki: &Yuki, view: &GitView, cx: &mut Context<Yuki>) -> impl IntoElement {
    let (fg, red) = (cx.theme().foreground, cx.theme().red);
    let rows = view.conflicts.iter().enumerate().map(|(ix, c)| {
        let (path, ours, theirs) = (c.path.clone(), c.ours.clone(), c.theirs.clone());
        let path2 = path.clone();
        h_flex()
            .px_3()
            .h(px(24.))
            .gap_2()
            .child(div().w(px(10.)).text_color(red).child("!"))
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .text_color(fg)
                    .child(yuki.git_label(c.node_id.as_deref(), &c.path)),
            )
            .child(link(("mine", ix), "mine", cx).on_click(cx.listener(
                move |this, _, window, cx| this.git_resolve(path.clone(), ours.clone(), window, cx),
            )))
            .child(link(("theirs", ix), "theirs", cx).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.git_resolve(path2.clone(), theirs.clone(), window, cx)
                },
            )))
    });
    v_flex()
        .child(section(
            t!("conflicts %{n}", n = view.conflicts.len()).into_owned(),
            cx,
        ))
        .children(rows.collect::<Vec<_>>())
}

fn history(view: &GitView, cx: &mut Context<Yuki>) -> impl IntoElement {
    let (fg, faint) = (cx.theme().foreground, ui::faint(cx));
    let merging = view.info.merging && view.conflicts.is_empty();
    let rows = view.log.iter().map(|c| {
        h_flex()
            .px_3()
            .h(px(22.))
            .gap_2()
            .child(div().text_color(faint).child(c.short_id.clone()))
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .text_color(fg)
                    .child(c.summary.clone()),
            )
            .child(div().text_color(faint).child(ago(c.timestamp)))
    });
    v_flex()
        // Every conflict settled: the merge still needs its commit.
        .when(merging, |s| {
            s.child(
                h_flex().px_3().pt_3().child(
                    Button::new("git-finish")
                        .small()
                        .outline()
                        .label(t!("finish merge").into_owned())
                        .on_click(
                            cx.listener(|this, _, window, cx| this.git_finish_merge(window, cx)),
                        ),
                ),
            )
        })
        .child(section(t!("history").into_owned(), cx))
        .children(rows.collect::<Vec<_>>())
        .when(view.log.is_empty(), |s| {
            s.child(div().px_3().text_color(faint).child(t!("no commits yet")))
        })
}
