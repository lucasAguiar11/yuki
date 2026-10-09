//! Window chrome: breadcrumb title bar, `:command` status line, welcome screen.

use gpui_kit::component::{
    button::Button, h_flex, select::Select, tooltip::Tooltip, v_flex, ActiveTheme as _, IconName,
    Sizable as _, TitleBar,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{
    ui, workspace::Yuki, ImportVoleeo, OpenCommands, OpenEnvs, OpenRequests, OpenSettings,
    OpenWorkspaces, ToggleEditor, ToggleGit, ToggleSidebar, ToggleVars,
};

/// `demo / JSONPlaceholder / photos                         env [Local ▾]`
pub fn title_bar(yuki: &Yuki, cx: &mut Context<Yuki>) -> impl IntoElement {
    let theme = cx.theme();
    let (fg, hover_bg) = (theme.foreground, theme.list_hover);
    let crumbs = yuki.breadcrumb(cx);
    let last = crumbs.len().saturating_sub(1);
    let mut trail = h_flex()
        .gap_1p5()
        .text_sm()
        .text_color(theme.muted_foreground);
    for (i, crumb) in crumbs.into_iter().enumerate() {
        if i > 0 {
            trail = trail.child(div().text_color(ui::faint(cx)).child("/"));
        }
        let item = div().id(("crumb", i)).child(crumb);
        trail = trail.child(match i {
            // The workspace segment opens the workspace switcher.
            0 => item
                .cursor_pointer()
                .hover(|s| s.text_color(theme.foreground))
                .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenWorkspaces), cx)),
            i if i == last => item.text_color(theme.foreground),
            _ => item,
        });
    }
    TitleBar::new().child(
        h_flex()
            .w_full()
            .pr_3()
            .justify_between()
            .child(trail)
            .child(
                h_flex()
                    .gap_2()
                    .children(yuki.env.as_ref().map(|env| {
                        let fg = theme.foreground;
                        let color = yuki.active_env_color(cx);
                        h_flex()
                            .gap_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(
                                // `● Local ▾` — tinted with the env's color, so the
                                // active environment reads at a glance.
                                h_flex()
                                    .w(px(170.))
                                    .pl_2()
                                    .gap_1()
                                    .rounded(theme.radius)
                                    .border_1()
                                    .border_color(
                                        color.map_or(transparent_black(), |c| c.opacity(0.5)),
                                    )
                                    .when_some(color, |s, c| s.bg(c.opacity(0.12)))
                                    .child(match color {
                                        Some(c) => div()
                                            .size(px(8.))
                                            .rounded_full()
                                            .bg(c)
                                            .into_any_element(),
                                        None => div().child("env").into_any_element(),
                                    })
                                    .child(
                                        div()
                                            .flex_1()
                                            .child(Select::new(env).small().appearance(false)),
                                    ),
                            )
                            .child(
                                div()
                                    .id("env-edit")
                                    .px_1()
                                    .cursor_pointer()
                                    .hover(move |s| s.text_color(fg))
                                    .child(t!("edit"))
                                    .on_click(|_, window, cx| {
                                        window.dispatch_action(Box::new(OpenEnvs), cx)
                                    }),
                            )
                    }))
                    .child(
                        // Mouse path to ⌘,; the tooltip teaches the shortcut.
                        div()
                            .id("open-settings")
                            .p_1()
                            .rounded(theme.radius)
                            .cursor_pointer()
                            .text_color(theme.muted_foreground)
                            .hover(move |s| s.text_color(fg).bg(hover_bg))
                            .child(IconName::Settings)
                            .tooltip(|window, cx| {
                                Tooltip::new(t!("settings ⌘,").into_owned()).build(window, cx)
                            })
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(OpenSettings), cx)
                            }),
                    ),
            ),
    )
}

/// Vim-flavoured hints, each one clickable: `:requests ⌘P  :commands ⌘⇧P …`
pub fn status_line(yuki: &Yuki, cx: &mut Context<Yuki>) -> impl IntoElement {
    let theme = cx.theme();
    let commands: [(&str, &str, Box<dyn Action>); 8] = [
        (":requests", "⌘P", Box::new(OpenRequests)),
        (":workspace", "⌘O", Box::new(OpenWorkspaces)),
        (":commands", "⌘⇧P", Box::new(OpenCommands)),
        (":sidebar", "⌘B", Box::new(ToggleSidebar)),
        (":request", "⌘E", Box::new(ToggleEditor)),
        (":vars", "⌘⇧E", Box::new(ToggleVars)),
        (":envs", "⌘⌥E", Box::new(OpenEnvs)),
        (":git", "⌘⇧G", Box::new(ToggleGit)),
    ];
    // `⎇ main · 3 ↑1` — branch, uncommitted changes, unpushed commits.
    let git = yuki.git.view.as_ref().filter(|v| v.info.is_repo).map(|v| {
        let mut chip = format!("⎇ {}", v.info.branch.as_deref().unwrap_or("?"));
        if !v.changes.is_empty() {
            chip += &format!(" · {}", v.changes.len());
        }
        if v.info.ahead > 0 {
            chip += &format!(" ↑{}", v.info.ahead);
        }
        if v.info.behind > 0 {
            chip += &format!(" ↓{}", v.info.behind);
        }
        let fg = theme.foreground;
        div()
            .id("git-chip")
            .cursor_pointer()
            .hover(move |s| s.text_color(fg))
            .child(chip)
            .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleGit), cx))
    });
    h_flex()
        .h(px(24.))
        .px_3()
        .gap_4()
        .text_xs()
        .font_family(theme.mono_font_family.clone())
        .text_color(theme.muted_foreground)
        .bg(theme.status_bar)
        .border_t_1()
        .border_color(theme.status_bar_border)
        .children(
            commands
                .into_iter()
                .enumerate()
                .map(|(i, (name, key, action))| {
                    h_flex()
                        .id(("cmd", i))
                        .gap_1()
                        .cursor_pointer()
                        .hover(|s| s.text_color(theme.foreground))
                        .child(t!(name))
                        .child(div().text_color(ui::faint(cx)).child(key))
                        .on_click(move |_, window, cx| {
                            window.dispatch_action(action.boxed_clone(), cx)
                        })
                }),
        )
        .child(div().flex_1())
        .child(yuki.status.clone())
        .children(git)
}

/// Empty center: the app name and the handful of commands that matter.
pub fn welcome(has_workspaces: bool, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let rows: [(&str, &str, &str); 5] = [
        (":requests", "⌘P", "open a request"),
        (":workspace", "⌘O", "switch workspace"),
        (":commands", "⌘⇧P", "all actions"),
        (":send", "⌘↵", "send the active request"),
        (":sidebar", "⌘B", "show/hide the tree"),
    ];
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_6()
        .child(
            v_flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .text_size(px(28.))
                        .font_weight(FontWeight::BOLD)
                        .child("yuki"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(t!("API console")),
                ),
        )
        .child(
            v_flex()
                .gap_1p5()
                .text_sm()
                .children(rows.map(|(cmd, key, desc)| {
                    h_flex()
                        .gap_4()
                        .child(div().w(px(110.)).child(t!(cmd)))
                        .child(
                            div()
                                .w(px(48.))
                                .text_color(theme.muted_foreground)
                                .child(key),
                        )
                        .child(div().text_color(theme.muted_foreground).child(t!(desc)))
                })),
        )
        .when(!has_workspaces, |s| {
            s.child(
                Button::new("welcome-import")
                    .small()
                    .outline()
                    .label(t!("Import workspaces from Voleeo").into_owned())
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ImportVoleeo), cx)),
            )
        })
}
