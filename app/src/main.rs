#[macro_use]
extern crate rust_i18n;

// UI copy is English in code; `t!("text")` looks it up in `locales/<lang>.yml`
// and falls back to the English text itself when there's no translation.
i18n!("locales", fallback = "en");

mod about;
mod auth;
mod auth_form;
mod backend;
mod body;
mod chrome;
mod code_vars;
mod complete;
mod curl;
mod document;
mod draft;
mod editor;
mod env_screen;
mod env_view;
mod envs;
mod export_view;
mod git;
mod git_ops;
mod git_panel;
mod import_render;
mod import_view;
mod items;
mod mcp;
mod moving;
mod notes;
mod oauth;
mod palette;
mod prefs;
mod rename;
mod response;
mod secrets;
mod sent;
mod settings;
mod settings_view;
mod sidebar;
mod table;
mod tabs;
mod transfer;
mod transfer_ui;
mod tree;
mod ui;
mod update;
mod var_view;
mod vars;
mod vars_panel;
mod workspace;
mod ws_folder;

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
        OpenSettings,
        CloseSettings,
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
        CopyAsCurl,
        CancelRename,
        OpenAbout,
        CheckUpdates,
        InstallUpdate,
        OpenFolder,
        ReportBug,
        ImportCollection,
        ExportWorkspace,
        CloseTransfer
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
        // ⌘, on macOS, ctrl-, elsewhere.
        KeyBinding::new("secondary-,", OpenSettings, None),
        KeyBinding::new("escape", CloseSettings, Some("SettingsScreen")),
        KeyBinding::new("escape", CloseTransfer, Some("Transfer")),
        KeyBinding::new("cmd-s", Save, None),
        KeyBinding::new("cmd-enter", Send, None),
        KeyBinding::new("cmd-shift-c", CopyAsCurl, None),
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
    // `sw_vers` runs on click, not on every render of the about screen.
    cx.on_action(|_: &ReportBug, cx| cx.open_url(&about::bug_url()));
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

gpui_kit::assets::icon_assets!(ExtraIcons, [LocateFixed, PawPrint]);

/// The kit's default icon bundle plus the few catalog icons Yuki uses beyond
/// it (`AllAssets` would embed the whole 7 MB catalog).
struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        let own: &'static [u8] = match path {
            "logo.svg" => include_bytes!("../assets/logo.svg"),
            "sleep.svg" => include_bytes!("../assets/sleep.svg"),
            _ => &[],
        };
        if !own.is_empty() {
            return Ok(Some(own.into()));
        }
        match ExtraIcons.load(path)? {
            Some(svg) => Ok(Some(svg)),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut all = gpui_kit::assets::Assets.list(path)?;
        all.extend(ExtraIcons.list(path)?);
        Ok(all)
    }
}

/// Dock icon for `cargo run`, which has no bundle to take it from.
#[cfg(target_os = "macos")]
fn set_dock_icon() {
    use objc2::AllocAnyThread as _;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{MainThreadMarker, NSData};
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(include_bytes!("../assets/icon.png"));
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        // SAFETY: main thread, valid image; AppKit retains it.
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
    }
}

fn main() {
    // `yuki mcp`: the stdio bridge an MCP client launches; no window.
    #[cfg(unix)]
    if std::env::args().nth(1).as_deref() == Some("mcp") {
        if let Err(e) = mcp::bridge() {
            eprintln!("yuki mcp: {e}");
            std::process::exit(1);
        }
        return;
    }
    let (notify, agent_edits) = mcp::changes();
    let backend = match backend::Backend::open(notify) {
        Ok(b) => b,
        Err(e) => {
            eprintln!(
                "yuki: failed to open data dir {}: {e}",
                backend::data_dir().display()
            );
            std::process::exit(1);
        }
    };
    let mcp = mcp::Link {
        edits: agent_edits,
        clients: mcp::start(&backend),
    };
    gpui_kit::application().with_assets(Assets).run(move |cx| {
        gpui_kit::init(cx);
        apply_theme(cx);
        prefs::Prefs::init(cx);
        cx.set_global(update::Updater::default());
        bind_keys(cx);
        register_graphql();
        #[cfg(target_os = "macos")]
        set_dock_icon();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1280.), px(800.)), cx)),
            ..TitleBar::window_options()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| workspace::Yuki::new(backend, mcp, window, cx))
        })
        .expect("failed to open window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod locales_test;
