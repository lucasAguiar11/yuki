---
name: yuki-gpui
description: Use when writing or changing Yuki UI code in app/src (GPUI via gpui-kit 0.7) — views, actions/keybindings, panels, palettes, inputs/chips, theme tokens, async loading — or debugging GPUI render, focus, borrow or layout problems.
---

# Yuki GPUI

Patterns verified against `gpui-kit 0.7.1` (`gpui-component`/`gpui-base` 0.7.1, `gpui-pre` 0.3.8). APIs differ from upstream Zed `gpui` — **grep the source before guessing**:

```bash
R=~/.cargo/registry/src/index.crates.io-*/
grep -rn "pub fn <name>" $R/gpui-component-0.7.1/src $R/gpui-base-0.7.1/src $R/gpui-pre-0.3.8/src
```

## Imports

```rust
use gpui_kit::{prelude::FluentBuilder as _, *};           // gpui core + .when/.map
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, IconName, Sizable as _, ...};
```

## Shapes used here

| Need | Use |
|---|---|
| Root view | `workspace::Yuki` (owns tabs, panels, palette, env select) |
| One request | `Entity<RequestEditor>` (`editor.rs` state, `document.rs` Render) |
| Action | `actions!` in `main.rs` → `bind_keys` → `.on_action(cx.listener(...))` in `Yuki::render` |
| Event from child | `impl EventEmitter<E>` + `cx.subscribe(&entity, ...)` (ex.: `EditorChanged`) |
| Input events | `cx.subscribe_in(&input, window, \|this, input, ev: &InputEvent, window, cx\| ...)` |
| Blocking I/O | `let t = cx.background_spawn(async move { backend.x() }); cx.spawn_in(window, async move \|this, cx\| { let v = t.await; this.update_in(cx, \|this, window, cx\| ...) }).detach();` |
| Network | `backend.save_and_send(req, env).await` (tokio runtime inside `Backend`) |
| Clipboard | `cx.write_to_clipboard(ClipboardItem::new_string(s))` |
| Tooltip | `.id(..).tooltip(move \|w, cx\| Tooltip::new(text.clone()).build(w, cx))`; rich: `Tooltip::element(\|_, cx\| ...)` |
| Selectable text | `gpui_kit::base::SelectableText::new(id, text).document_order(n)` per line (window-wide selection, ⌘C via Root); clicks inside it start a selection, so put buttons outside (`sent.rs`) |
| Split panes | `h_resizable(id)` / `v_resizable` + `resizable_panel().size(px).size_range(a..b).flex_none()` |
| Palette | `palette::Palette` (wraps `Command`/`CommandState`; `Target` decides confirm) |
| Select | `SelectState::new(SearchableVec::new(Vec<SharedString>), Some(IndexPath::new(i)), window, cx)` + `SelectEvent` |
| Context menu | `.id(..).context_menu(\|m, _, _\| m.item(PopupMenuItem::new("x").on_click(...)))` — never nest two triggers (both open) |
| Confirm dialog | `window.open_alert_dialog(cx, \|a, _, _\| a.title(..).show_cancel(true).ok_variant(ButtonVariant::Danger).on_ok(..))` (see `items::confirm_delete`) |
| Drag & drop | `.on_drag(payload, \|d, grab, _, cx\| cx.new(..preview view..))` + `.drag_over::<T>(style)` + `.on_drop(cx.listener(..))` (`moving::Dragged`) |
| Tree mutation | `Yuki::mutate(rename, op)` — store op off-thread, then reload + resync open tabs |
| Text inputs | `InputState` (1 line) · `TextareaState` (multi) · `EditorState::new(..).language("json")` (code; **no tokens**) |
| Highlighting | languages = gpui-kit `tree-sitter-*` features in `app/Cargo.toml` (json, html, graphql). XML uses `"html"`. GraphQL ships an empty query → ours in `app/src/graphql.scm`, registered in `main::register_graphql` |
| Chips | `state.set_value(vars::content(text), window, cx)`; renderer `Input::new(&s).token(\|ctx, w, cx\| ...)` |
| Icons | `IconName::X` — only the default set (`gpui-kit-assets-0.7.1/default-icons.txt`: chevron-*, close, copy, eye, eye-off, search, panel-left, chevrons-up-down, …) |

## Adding a user-facing action (checklist)

1. Name it in `actions!` (`main.rs`) and bind a key in `bind_keys` (check for clashes; macOS ⌘⇧V/⌘⇧W are taken).
2. Handle it in `Yuki::render` (`.on_action`), delegating to the active `RequestEditor` when it is per-request.
3. Expose it: command palette (`open_palette` → `Kind::Commands`), and if it is core, the status line (`chrome::status_line`) and welcome rows (`chrome::welcome`).
4. Copy in pt-BR, lowercase, short.

## Theme & typography

- Colors only from `cx.theme()`; semantic helpers in `ui.rs` (`status_color`, `method_color`, `faint`). New color → add a key to `app/themes/yuki-console.json` (`base.*` → `theme.red/green/blue/yellow/magenta/cyan`).
- Data text (URL, headers, JSON, methods, status line, meta line) → `.font_family(theme.mono_font_family.clone())` (Menlo). "SF Mono" is **not** a usable family name on macOS — check families with CoreText (`CTFontManagerCopyAvailableFontFamilyNames`) before picking a font.
- Contrast: never use `theme.border` for text; tertiary text is `ui::faint(cx)`.

## Pitfalls (each one cost a debugging session)

- **`cx.theme()` borrow**: it borrows `cx` immutably. Copy needed `Hsla`/`SharedString` into locals, or build `cx.listener`/`&mut cx` children *before* taking `theme`. Same for `entity.read(cx)` held across a `&mut cx` call — clone what you need (e.g. `editor.vars.clone()` is an `Rc`).
- **`focus` + `read`**: `e.read(cx).focus(window, cx)` is a double borrow; get the `FocusHandle` first, then `.focus(window, cx)`.
- **No focus = dead keyboard**: after closing tabs/switching workspace call `Yuki::focus_active`.
- **Dialogs and focus**: a dialog opened from a palette/menu click loses focus to their restore (Enter does nothing, keys leak behind the modal) — open it in `window.defer`. On close it re-focuses whatever held focus when it opened; if that view is gone (deleted tab), the keyboard dies — park focus on `Yuki.focus` first.
- **Resizable**: no `.visible(false)` panels (stale size → jumping divider). Leave hidden panels out and key the group by visibility combo.
- **Render loops**: never notify unconditionally from an observer of a child that notifies on every host render (e.g. `CommandState`). Guard on a changed value.
- **Token hover**: `on_token_hover` + storing hover in view state breaks (re-render resets tracking). Use a tooltip on the chip element.
- **`Command` list** fills its `max_h`; size it from `matched_count` (`palette::list_height`, 28 px rows).
- **Tokio runtime ownership**: tasks spawned on `Backend.rt` must not own `Arc<Backend>`.
- **`#[macro_export]` macros** live at crate root: call as `crate::name!` / `$crate::...` inside.
- **`use gpui_kit::*` shadows `#[test]`** (gpui's own macro → "recursion limit" error): test modules import names explicitly, not `super::*`, when the parent globs gpui.

## Verify

Compile + clippy is not proof. Run the app and look at it — use skill `yuki-visual-smoke`.
