mod backend;
mod chrome;
mod document;
mod draft;
mod editor;
mod palette;
mod response;
mod sidebar;
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
        Quit
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
