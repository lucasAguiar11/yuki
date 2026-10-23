//! Window chrome: breadcrumb title bar, `:command` status line, welcome screen.

use gpui_kit::component::{
    button::Button,
    h_flex,
    menu::{ContextMenuExt as _, PopupMenuItem},
    select::Select,
    tooltip::Tooltip,
    v_flex, ActiveTheme as _, Icon, IconName, Sizable as _, TitleBar,
};
use gpui_kit::{prelude::FluentBuilder as _, *};

use crate::{
    ui,
    update::{State, Updater},
    workspace::Yuki,
    ImportCollection, ImportVoleeo, InstallUpdate, NewRequest, OpenCommands, OpenEnvs, OpenMcp,
    OpenRequests, OpenSettings, OpenWorkspaces, ToggleEditor, ToggleGit, ToggleLayout,
    ToggleSidebar, ToggleVars,
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
                        let (fg, faint) = (theme.foreground, ui::faint(cx));
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
                                h_flex()
                                    .id("env-edit")
                                    .px_1()
                                    .gap_1()
                                    .cursor_pointer()
                                    .hover(move |s| s.text_color(fg))
                                    .child(t!("environments"))
                                    .child(div().text_color(faint).child("⌘⌥E"))
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
    // `● mcp 1` — server up (green) or down (red), agents connected.
    let (dot, tip) = match yuki.mcp {
        Some(0) => (theme.green, t!("MCP ready · no agent connected")),
        Some(n) => (theme.green, t!("MCP · agents connected: %{n}", n = n)),
        None => (theme.red, t!("MCP is off — the socket didn't open")),
    };
    let tip = SharedString::from(tip.into_owned());
    let fg = theme.foreground;
    let mcp = h_flex()
        .id("mcp-chip")
        .gap_1()
        .cursor_pointer()
        .hover(move |s| s.text_color(fg))
        .child(div().text_color(dot).child("●"))
        .child(match yuki.mcp {
            Some(n) if n > 0 => format!("mcp {n}"),
            _ => "mcp".into(),
        })
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenMcp), cx));
    // `↑ v0.2.0` — a newer release; click installs it and restarts.
    let updater = cx.global::<Updater>();
    let release = updater.release.as_ref().map(|r| {
        let (color, tip) = match &updater.state {
            State::Failed(e) => (theme.red, e.clone()),
            _ => (
                theme.blue,
                t!(
                    "Yuki %{v} is out — click to install and restart",
                    v = r.version
                )
                .into_owned()
                .into(),
            ),
        };
        let fg = theme.foreground;
        div()
            .id("update-chip")
            .cursor_pointer()
            .text_color(color)
            .hover(move |s| s.text_color(fg))
            .child(if updater.state == State::Installing {
                t!("updating…").into_owned()
            } else {
                format!("↑ {}", r.version)
            })
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_click(|_, window, cx| window.dispatch_action(Box::new(InstallUpdate), cx))
    });
    // Response below / beside the request: the current one lit, the other
    // switches. Only while a request is open — it's that view's layout.
    let beside = cx.global::<crate::prefs::Prefs>().response_beside;
    let (fg, muted, lit, radius) = (
        theme.foreground,
        theme.muted_foreground,
        theme.secondary,
        theme.radius,
    );
    let layout = |id: &'static str, icon: IconName, on: bool, tip: String| {
        let current = on == beside;
        div()
            .id(id)
            .p_0p5()
            .rounded(radius)
            .cursor_pointer()
            .text_color(if current { fg } else { muted })
            .when(current, |d| d.bg(lit))
            .hover(move |s| s.text_color(fg))
            .child(Icon::new(icon).xsmall())
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_click(move |_, window, cx| {
                if !current {
                    window.dispatch_action(Box::new(ToggleLayout), cx)
                }
            })
    };
    let layouts = yuki.active_editor().is_some().then(|| {
        h_flex()
            .gap_0p5()
            .child(layout(
                "layout-below",
                IconName::PanelBottom,
                false,
                t!("response below · ⌘⌥L").into_owned(),
            ))
            .child(layout(
                "layout-beside",
                IconName::PanelRight,
                true,
                t!("response beside · ⌘⌥L").into_owned(),
            ))
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
        .children(layouts)
        .children(git)
        .child(mcp)
        .children(release)
}

/// Empty center: the logo and the workspaces, one click to switch (⌘-click or
/// right-click: in a new window).
pub fn welcome(yuki: &Yuki, cx: &mut Context<Yuki>) -> impl IntoElement {
    let theme = cx.theme();
    let (fg, muted, hover_bg, mark, radius) = (
        theme.foreground,
        theme.muted_foreground,
        theme.list_hover,
        theme.ring,
        theme.radius,
    );
    let current = yuki.ws.as_ref().map(|w| w.id.as_str());
    let rows: Vec<_> = yuki
        .workspaces
        .iter()
        .map(|ws| {
            let active = current == Some(ws.id.as_str());
            let (pick, id) = (ws.clone(), ws.id.clone());
            h_flex()
                .id(SharedString::from(format!("welcome-ws-{}", ws.id)))
                .h(px(28.))
                .px_2()
                .gap_2()
                .rounded(radius)
                .cursor_pointer()
                .text_color(if active { fg } else { muted })
                .hover(move |s| s.bg(hover_bg).text_color(fg))
                .child(div().w(px(14.)).flex_none().when(active, |d| {
                    d.child(
                        Icon::new(assets::IconName::PawPrint)
                            .small()
                            .text_color(mark),
                    )
                }))
                .child(div().flex_1().truncate().child(ws.name.clone()))
                .when(!active, |row| {
                    row.on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                        if ev.modifiers().platform {
                            let id = pick.id.clone();
                            cx.defer(move |cx| crate::windows::open_new(id, cx));
                        } else {
                            this.open_workspace(pick.clone(), window, cx)
                        }
                    }))
                })
                .context_menu(move |m, _, _| {
                    let id = id.clone();
                    m.item(
                        PopupMenuItem::new(t!("open in new window").into_owned())
                            // This window already shows it.
                            .disabled(active)
                            .on_click(move |_, _, cx| {
                                let id = id.clone();
                                cx.defer(move |cx| crate::windows::open_new(id, cx));
                            }),
                    )
                })
        })
        .collect();
    // With a workspace open, the first moves: `nova request ⌘N` …
    let starts = yuki.ws.is_some().then(|| {
        let start =
            |id: &'static str, label: String, key: &'static str, action: Box<dyn Action>| {
                h_flex()
                    .id(id)
                    .h(px(28.))
                    .px_2()
                    .rounded(radius)
                    .cursor_pointer()
                    .text_color(muted)
                    .hover(move |s| s.bg(hover_bg).text_color(fg))
                    .child(div().flex_1().child(label))
                    .child(div().text_xs().child(key))
                    .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
            };
        v_flex()
            .w(px(320.))
            .text_sm()
            .child(start(
                "welcome-new",
                t!("new request").into_owned(),
                "⌘N",
                Box::new(NewRequest),
            ))
            .child(start(
                "welcome-open",
                t!("open a request").into_owned(),
                "⌘P",
                Box::new(OpenRequests),
            ))
            .child(start(
                "welcome-cmds",
                t!("every command").into_owned(),
                "⌘⇧P",
                Box::new(OpenCommands),
            ))
    });
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_6()
        .child(
            v_flex()
                .items_center()
                .gap_1()
                // gpui rasterizes an SVG at its intrinsic size ×2 (72 in logo.svg) and
                // aliases when scaling a bigger raster down — keep the two in step.
                .child(img("logo.svg").size(px(72.)).mb_2())
                .child(
                    div()
                        .text_size(px(28.))
                        .font_weight(FontWeight::BOLD)
                        .child("yuki"),
                )
                .child(div().text_sm().text_color(muted).child(t!("API console"))),
        )
        .children(starts)
        .when(!rows.is_empty(), |s| {
            s.child(
                v_flex()
                    .w(px(320.))
                    .gap_1()
                    .child(
                        h_flex()
                            .px_2()
                            .justify_between()
                            .text_xs()
                            .text_color(muted)
                            .child(t!("workspaces"))
                            .child("⌘O"),
                    )
                    .child(
                        v_flex()
                            .id("welcome-workspaces")
                            .max_h(px(280.))
                            .overflow_y_scroll()
                            .text_sm()
                            .children(rows),
                    ),
            )
        })
        .child(
            div()
                .id("welcome-import-collection")
                .text_xs()
                .cursor_pointer()
                .text_color(muted)
                .hover(move |s| s.text_color(fg))
                .child(t!("import a collection (Postman, Insomnia, Yaak, Voleeo)…").into_owned())
                .on_click(|_, window, cx| window.dispatch_action(Box::new(ImportCollection), cx)),
        )
        .when(yuki.workspaces.is_empty(), |s| {
            s.child(
                Button::new("welcome-import")
                    .small()
                    .outline()
                    .label(t!("Import workspaces from Voleeo").into_owned())
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ImportVoleeo), cx)),
            )
        })
}
