mod auth;
mod auth_form;
mod backend;
mod body;
mod chrome;
mod code_vars;
mod complete;
mod document;
mod draft;
mod editor;
mod env_screen;
mod env_view;
mod envs;
mod git;
mod git_ops;
mod git_panel;
mod items;
mod moving;
mod oauth;
mod palette;
mod rename;
mod response;
mod secrets;
mod sent;
mod sidebar;
mod table;
mod tabs;
mod tree;
mod ui;
mod var_view;
mod vars;
mod vars_panel;
mod workspace;

use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeMode, ThemeRegistry, TitleBar};
use gpui_kit::*;

actions!(
    yuki,
    [
        ToggleSidebar,
        Save,
        Send,
        CloseTab,
        NextTab,
        PrevTab,
        OpenRequests,
        OpenCommands,
        OpenWorkspaces,
        ImportVoleeo,
        ToggleEditor,
        ToggleVars,
        ToggleGit,
        OpenEnvs,
        CloseEnvs,
        Quit,
        FocusNext,
        FocusPrev,
        CompleteNext,
        CompletePrev,
        CompleteAccept,
        CompleteDismiss,
        CompleteOpen,
        NewRequest,
        NewFolder,
        RenameItem,
        DuplicateItem,
        MoveItem,
        DeleteItem,
        CancelRename
    ]
);

const THEME: &str = include_str!("../themes/yuki-console.json");

fn apply_theme(cx: &mut App) {
    let registry = ThemeRegistry::global_mut(cx);
    if let Err(e) = registry.load_themes_from_str(THEME) {
        eprintln!("[theme] {e}");
        return;
    }
    if let Some(config) = registry.themes().get("Yuki Console").map(Rc::clone) {
        Theme::global_mut(cx).apply_config(&config);
    }
    Theme::change(ThemeMode::Dark, None, cx);
}

fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-e", ToggleEditor, None),
        KeyBinding::new("cmd-shift-e", ToggleVars, None),
        KeyBinding::new("cmd-shift-g", ToggleGit, None),
        KeyBinding::new("cmd-alt-e", OpenEnvs, None),
        KeyBinding::new("escape", CloseEnvs, Some("EnvScreen")),
        KeyBinding::new("cmd-s", Save, None),
        KeyBinding::new("cmd-enter", Send, None),
        // Inputs bind cmd-enter themselves (newline + PressEnter); same-context
        // bindings registered later win, so this makes ⌘↵ send from any field.
        KeyBinding::new("cmd-enter", Send, Some("Input")),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
        KeyBinding::new("cmd-p", OpenRequests, None),
        KeyBinding::new("cmd-shift-p", OpenCommands, None),
        KeyBinding::new("cmd-o", OpenWorkspaces, None),
        KeyBinding::new("cmd-q", Quit, None),
        // Single-line inputs propagate tab, so it walks the table cells.
        KeyBinding::new("tab", FocusNext, None),
        KeyBinding::new("shift-tab", FocusPrev, None),
        // Same depth as the Input's own keys, registered later: these win
        // while the variable popup is open (see `complete.rs`).
        KeyBinding::new("down", CompleteNext, Some("VarComplete > Input")),
        KeyBinding::new("up", CompletePrev, Some("VarComplete > Input")),
        KeyBinding::new("enter", CompleteAccept, Some("VarComplete > Input")),
        KeyBinding::new("tab", CompleteAccept, Some("VarComplete > Input")),
        KeyBinding::new("escape", CompleteDismiss, Some("VarComplete > Input")),
        // ⌘space never reaches the app: macOS hands it to Spotlight.
        KeyBinding::new("ctrl-space", CompleteOpen, Some("Input")),
        KeyBinding::new("cmd-n", NewRequest, None),
        KeyBinding::new("cmd-shift-n", NewFolder, None),
        // Same trick as the popup keys: beats the Input's own escape.
        KeyBinding::new("escape", CancelRename, Some("Rename > Input")),
    ]);
    // Fallback when nothing is focused; the root view handles Quit with a flush.
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
}

/// gpui-component ships the GraphQL grammar with an empty highlight query;
/// give it ours (`graphql.scm`) so query bodies get colors.
fn register_graphql() {
    let registry = gpui_kit::component::highlighter::LanguageRegistry::singleton();
    if let Some(mut config) = registry.language("graphql") {
        config.highlights = include_str!("graphql.scm").into();
        registry.register("graphql", &config);
    }
}

fn main() {
    let backend = match backend::Backend::open() {
        Ok(b) => b,
        Err(e) => {
            eprintln!(
                "yuki: failed to open data dir {}: {e}",
                backend::data_dir().display()
            );
            std::process::exit(1);
        }
    };
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            apply_theme(cx);
            bind_keys(cx);
            register_graphql();
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1280.), px(800.)), cx)),
                ..TitleBar::window_options()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| workspace::Yuki::new(backend, window, cx))
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
