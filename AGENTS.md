# AGENTS.md — Yuki

Guide for agents in this repository. **Keep it under 200 lines**: when you add a rule, trim another.

## Product

Yuki is a **native API console** (Rust + GPUI via `gpui-kit`), inspired by Voleeo and Zed — a copy of neither. "Console" visual direction: one document per request (prompt line `GET ▸ url`), monospace wherever there is data, color only where it carries meaning, keyboard-first. UI copy is **English**, lowercase and short (`no response`, `▸ request ⌘E`), translated through `app/locales/` (see Localization).

Pre-release: no migration shims for internal formats. **Exception:** on-disk data (workspace/request/env YAML) belongs to the user — keep it readable (serde `default`/`rename`), never corrupt it.

## License and business model (decided — don't change without being asked)

- **Code: MIT, 100% open.** Same model as Yaak: what's charged is *commercial use of the official binaries* (terms in `docs/legal/TERMS.md`, Portuguese in `TERMS.pt-BR.md`), never features. Forbidden: feature paywalls, closed modules, changing the code license, locking the app when the trial ends.
- `LICENSE` carries Voleeo's copyright (required: `crates/yuki-*` derive from it) and Yuki's; provenance in `NOTICE`. Never remove the Voleeo line.
- Visible strings say **Yuki** (UI, default User-Agent, MCP `serverInfo`, OAuth pages, import messages). "Voleeo" only for origin/import and the `VoleeoBundle` format.
- New dependency: nothing GPL/LGPL/AGPL (check with `cargo metadata`). Distributed binaries must ship third-party licenses (`cargo about generate`).

## Layout

```
app/              `yuki` binary (GPUI) — the only layer that knows gpui
  themes/yuki-console.json   palette + syntax (single source of color)
  assets/        logo.svg (README, welcome) · sleep.svg (empty response) · icon.png dock (1024, Apple grid); SVG intrinsic size = render size
  locales/pt-BR.yml          translations (English source text → pt-BR)
  src/main.rs     i18n!, actions!, keybindings, theme, window
  src/workspace.rs  root view `Yuki`: workspaces, tabs, panels, palettes, actions
  src/editor.rs   `RequestEditor` (state/logic of one tab)
  src/document.rs RequestEditor render: prompt, meta line, request editor
  src/body.rs     body panel: type select + per-type input (raw, form/multipart, binary, graphql)
  src/table.rs    name/value table (headers and form/multipart fields); last row always blank
  src/complete.rs `{{ VAR }}` autocomplete (on typing `{{` or ⌃space) in URL, cells and code editors
  src/code_vars.rs `{{ VAR }}` "badge" in code editors (decorations; `EditorState` doesn't take tokens)
  src/response.rs status strip (time, local clock), body/headers, history (●●●)
  src/sent.rs     timeline tab: resolved request as sent + engine event log (credentials masked)
  src/var_view.rs `{{ VAR }}` chips + tooltip; src/vars.rs model/tokenization (pure)
  src/vars_panel.rs ⌘⇧E panel;  src/sidebar.rs, tabs.rs, chrome.rs, palette.rs
  src/env_screen.rs "environments" screen ⌘⌥E (state, autosave, create/rename/delete); env_view.rs render; envs.rs title-bar `env` select
  src/settings.rs "settings" screen ⌘, (workspace name, DNS overrides as hosts lines, shell-env allowlist); settings_view.rs render; prefs.rs font sizes + language
  src/items.rs    tree CRUD (create/duplicate/delete); moving.rs drag & drop + "move to…"; rename.rs inline rename
  src/backend.rs  bridge to the engine: tokio runtime, load/save/send, Voleeo import
  src/auth.rs     auth schemes as data (fields per type) + AuthConfig⇄form via serde; auth_form.rs auth tab; oauth.rs "get token"; secrets.rs secret crypto
  src/draft.rs, tree.rs, ui.rs  pure helpers
crates/yuki-*     engine inherited from Voleeo (MIT), no gpui/tauri
  yuki-core types+errors · yuki-storage YAML · yuki-http executor · yuki-mcp ApiBackend+resolve
  yuki-crypto AES-GCM/keychain · yuki-auth/oauth/cookies/ws/grpc/import/export/git
```

**Invariants:** `crates/` never depends on gpui. The whole send pipeline goes through `ApiBackend::send_http` (`crates/yuki-mcp/src/api/request/send.rs`) — app and MCP share it; never reimplement env/folder/auth/cookies/history in the UI. Variable resolution: `yuki_mcp::resolve` (`load_env_vars_from`, `apply_folder_vars`, `resolve_str`, `is_identifier`). `send_http` applies the workspace's DNS overrides and shell-env allowlist, but **not** workspace/folder headers or `inherit` auth (Voleeo's frontend resolved those) — a UI to edit them does nothing until the pipeline applies them.

## Localization

- Supported: `en` (source) and `pt-BR`. Code, comments, `AGENTS.md`, skills and commits are English. UI copy is English inside `t!("...")` (rust-i18n, gettext style): the English text **is** the key; every key goes into every `app/locales/<lang>.yml`.
- User-facing docs are bilingual: `README.md` ↔ `README.pt-BR.md`, `docs/legal/TERMS.md` ↔ `TERMS.pt-BR.md` — change both in the same commit.
- Any copy or doc change: skill `yuki-i18n` (rules, adding a language). Validate with `cargo test -p yuki locales_test` (missing, stale and divergent keys) and `.agents/skills/yuki-i18n/scripts/doc-parity.sh`.
- Language: `YUKI_LANG` (dev override) > settings → appearance (`settings.yaml` `language`) > system locale (`sys-locale`). Switching is live, but text cached at creation (input placeholders, select lists) only changes after a restart.

## Data

- Directory: `~/Library/Application Support/dev.yuki` (override `YUKI_DATA_DIR`). Same layout as Voleeo (`workspaces/{id}/req_*.yaml`, `envs-local/{id}/`, `keys/{id}.key`, `responses-local/`, machine-local `workspace-settings.yaml` — edit as a YAML tree, Voleeo keys ride along). App prefs: `settings.yaml`.
- Import: `backend::import_voleeo` **copies** (never links) workspaces, local envs and key files; skips `.git` and existing ids.
- Local-first: edits hit the disk 400 ms after the last keystroke (`editor::refresh_dirty` → `save`; one write at a time). ⌘S, send, closing a tab and quitting only bring it forward. Saving writes only if changed (`save_request_if_changed`); header ids are preserved by position (`draft::text_to_headers`) — fresh ids = phantom diff in the user's git.
- Encrypted workspace: auth secrets and secret env variables are opened on load, sealed on save (`secrets.rs`). An unchanged secret reuses the stored ciphertext — re-encrypting draws a new nonce = phantom diff.
- Environments: `shared: false` lives in `envs-local/` (never in git); `shared: true` goes to the workspace folder (gets committed). On the screen, never save rows that weren't loaded from the current environment (`EnvScreen::show` vs `select`) — it overwrites its variables.
- Keychain service: `yuki`. The key file is the fallback (and what import brings).
- Git: each `workspaces/{id}` is its own repository (Voleeo model), via `yuki-git` (libgit2, no git install needed). Panel ⌘⇧G (`git.rs` state/background ops, `git_ops.rs` actions, `git_panel.rs` UI). Pull conflict: pick one side's whole file; the disk never holds `<<<<<<<`.

## Commands

```bash
cargo run -p yuki                       # dev (deps at opt-level 3, app at 0)
YUKI_DATA_DIR=/tmp/yk cargo run -p yuki # isolated data for testing
YUKI_LANG=pt-BR cargo run -p yuki       # force a UI language
cargo build --release -p yuki
cargo test -p yuki -p yuki-mcp          # unit tests (app: draft, vars, locales)
cargo clippy -p yuki && cargo fmt --all
```

No packaged `.app` yet. CI (`.github/workflows/ci.yml`, macOS) runs fmt `--check`, clippy `--workspace --all-targets -D warnings` and `cargo test --workspace`.

## Code

- KISS/YAGNI; smallest diff that fixes the cause. Comment only the non-obvious (the *why*).
- `.rs` in `app/src`: target ≤ 250 lines; split by responsibility (`workspace.rs` is already over — split it when you touch it).
- Colors **only** via `cx.theme()` (JSON tokens) and `ui::{status_color, method_color, faint}`. Never hex in code. Data (URL, headers, methods, JSON, footer) uses `theme.mono_font_family`.
- Tests: only pure logic with real risk (`draft.rs`, `vars.rs`). UI is verified by running it (skill `yuki-visual-smoke`).

## GPUI / gpui-kit — traps already paid for

Details and patterns in the `yuki-gpui` skill. Summary of what cost the most:

1. **`cx` borrow:** `let theme = cx.theme()` borrows `cx`; copy the colors (`Hsla` is `Copy`) before any `cx.listener`/`&mut cx`, or build mutable children before taking `theme`.
2. **I/O off the UI:** YAML via `cx.background_spawn`; network via `Backend::save_and_send` (its own tokio runtime). The tokio task never captures the `Backend` (dropping the runtime inside it panics) — only `Arc<ApiBackend>`.
3. **Focus:** with no focused element no keybinding fires. After switching workspace/closing a tab, call `focus_active`.
4. **⌘↵ in inputs:** `Input` consumes `secondary-enter`; there's an extra `cmd-enter` binding with context `"Input"` registered later (it wins). Keep it.
5. **Resizable:** never `.visible(false)` on an `h_resizable` panel — a hidden panel keeps a stale size and the border "jumps" on drag. Omit the panel and use a group id per combination (`main-{sidebar}-{vars}`). Sized panels: `.flex_none()` + `size_range`.
6. **Hover on input tokens:** don't keep hover in view state (re-render resets tracking and hover dies). Use `.tooltip(...)` on the chip element.
7. **Palette (`Command`):** the list takes the whole `max_h`; height is computed from `matched_count` (`palette::list_height`) and the view only re-renders when the count changes (guarded observer — unconditional notify = loop).
8. **Template chips:** `vars::content` on load and `chip_templates!` on every `InputEvent::Change`; saved text stays literal (`{{ BASE }}`). `EditorState` (code) rejects tokens: there the badge is `RangeDecoration` + `TextDecoration` (`code_vars.rs`); inside a JSON string the syntax color beats the name color.
9. **Popup keys over `Input`:** `Input` consumes ↑↓↵⇥esc. To win, bind with context `"VarComplete > Input"` (same depth, registered later) and `key_context("VarComplete")` on the ancestor only while the popup is open. ⌘space belongs to Spotlight — it never reaches the app.
10. **Single-line cells:** `InputState` strips line breaks; `draft::cell_value` returns the saved value if the cell wasn't touched (avoids corrupting multi-line values).
11. **Theme-driven prefs:** mutate `Theme::global_mut(cx)` then `cx.refresh_windows()` (`prefs.rs`). Root sets the rem size from `theme.font_size` every frame, so text scales but `px(..)` row heights don't — the UI font is capped at 16. `mono_font_size` drives code editors and table cells.
12. **Icons:** `component::IconName` covers only gpui-kit's default bundle. Any other Lucide icon (`assets::IconName::X` via `Icon::new`) renders **blank, no error**, until it's added to `icon_assets!(ExtraIcons, [...])` in `main.rs`. Never `AllAssets` (embeds the 7 MB catalog).

## Delivery flow

1. Read the area + relevant skill. 2. Implement with the smallest diff. 3. `cargo build -p yuki`, `clippy`, tests. 4. **Run the app** and check the changed surface (skill `yuki-visual-smoke`) — compiling is not proof. 5. Clean up temporary fixtures/scripts.

Commits: Conventional Commits, in English, no AI co-authorship. Never commit/push without being asked.
