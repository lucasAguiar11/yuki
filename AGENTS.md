# AGENTS.md — Yuki

Guide for agents in this repository. **Keep it under 200 lines**: when you add a rule, trim another.

## Product

Yuki is a **native API console** (Rust + GPUI via `gpui-kit`), inspired by Voleeo and Zed — a copy of neither. "Console" visual direction: one document per request (prompt line `GET ▸ url`), monospace wherever there is data, color only where it carries meaning, keyboard-first. UI copy is **English**, lowercase and short (`no response`, `▸ request ⌘E`), translated through `app/locales/` (see Localization).

Pre-release: no migration shims for internal formats. **Exception:** on-disk data (workspace/request/env YAML) belongs to the user — keep it readable (serde `default`/`rename`), never corrupt it.

## License and business model (decided — don't change without being asked)

- **Code: MIT, 100% open.** Same model as Yaak: what's charged is *commercial use of the official binaries* (terms in `docs/legal/TERMS.md`, Portuguese in `TERMS.pt-BR.md`), never features. Forbidden: feature paywalls, closed modules, changing the code license, locking the app when the trial ends.
- `LICENSE` carries Voleeo's copyright (required: `crates/yuki-*` derive from it) and Yuki's; provenance in `NOTICE`. Never remove the Voleeo line.
- Visible strings say **Yuki** (UI, default User-Agent `yuki_core::DEFAULT_USER_AGENT`, MCP `serverInfo`, OAuth pages, import messages). "Voleeo" only for origin/import and the `VoleeoBundle` format.
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
  src/variation.rs request variations: `base ▾` chip in the meta line (switch/new/rename/delete); each variation is an independent copy of the five sendable fields (`Variation::copy_of`; older partial ones filled by `detach_variations` on load, in memory); the editor shows `saved.shown()` and `draft()` folds edits back into the active variation or the base (`draft::fold`); any change saves then rebuilds the editor
  src/body.rs     body panel: type select + per-type input (raw, form/multipart, binary, graphql)
  src/table.rs    name/value table (headers, form/multipart fields, query params); last row always blank
  src/params.rs   params tab (always in the meta line): path params (one row per URL `:name`, chipped in the URL) + query rows, all stored in `parameters`; the URL bar holds no query — a pasted one (or one left on blur) moves to the rows; `query.rs` normalizes requests that still have it in the URL (no write until edited)
  src/paste_curl.rs a `curl …` pasted into the URL bar overwrites the shown request (variation-aware, via `change`); curl.rs the reverse (copy as cURL); clip_curl.rs offers a clipboard cURL as `▸ import cURL` in the status line (checked on window activation, gated by `NSPasteboard.changeCount`; each content once; own copies never)
  src/complete.rs `{{` autocomplete (on `{{`, `$` or ⌃space), generic over `CompleteHost` (request editor, scope tab): variables, `resolve::funcs`, then latest-response JSON fields as `$response.body`; complete_view.rs the popup + keys
  src/response.rs status strip (time, local clock), body/headers, history (●●●); collapsed (⌘J) it's the strip under the editor, or a right-edge rail (`◂ 200`) when the response is beside; XML bodies indented by xml.rs; req_diff.rs the `diff` meta tab (offered while git marks the request changed): `req_{id}.yaml`/`.md` vs HEAD as git prints it, re-read on every `refresh_git`
  src/past.rs     picking an older history entry shows the request as it was then (read-only, in place of the editor) with restore/back; history stores it as `edited_request` (templates kept)
  src/capture.rs  response field → variable (pure: JSONPath under the caret, name, `$response.body` formula, readers); capture_view.rs the bar under a JSON response (⌥↵, name in place, readers ✕). The engine prefills `$response.body` (`resolve::prefill_responses`) before `resolve_str`
  src/sent.rs     response `log` tab: resolved request as sent + engine event log (credentials masked until `show tokens`); load_log.rs the same tab for a load run's answer (every send, ▸ request/response)
  src/req_settings.rs settings tab: timeout + redirects (`RequestSettings`, base-only, applied by yuki-http) and a load run (`ApiBackend::load_http`: resolved once, N sends P at a time, summary + one answer to history; `HttpResponse::load_log` keeps every send with its request/response events and body)
  src/var_view.rs `{{ VAR }}` chips + tooltip card, ⌘-click → origin (`vars::origin`: source request, folder tab, env row; missing = create); `$response.body` chips read `request › field` with the last value (`vars::Refs`, `Backend::refs`); src/vars.rs model/tokenization (pure)
  src/vars_panel.rs ⌘⇧E panel;  src/sidebar.rs, tabs.rs, chrome.rs, palette.rs
  src/env_screen.rs "environments" screen ⌘⌥E (state, autosave, create/rename/delete); env_view.rs render; envs.rs title-bar `env` select
  src/settings.rs "settings" screen ⌘, (workspace name, DNS overrides as hosts lines, shell-env allowlist, user-agent — the workspace `User-Agent` header row, absent = `send_http` adds `Yuki`; accept invalid certificates — machine-local `skipCertificateCheck`; sends then flag `⚠ unverified` via a `tls` timeline event); settings_view.rs render; prefs.rs font sizes + language; about.rs "about" section
  src/session.rs  `Session` global ↔ `session.yaml`: window bounds, panel visibility/sizes (`on_resize`), per workspace open tabs, collapsed folders, env; restored on launch and workspace switch, written by `windows::save_session` (quit/close)
  src/windows.rs  several windows, one `Yuki` root (workspace) each: ⌘⇧O / welcome ⌘-click open one; a workspace open elsewhere brings its window forward (`windows::activate`, checked in `open_workspace`); ⌘Q (`windows::quit`) flushes every window; MCP edits are a broadcast each window subscribes to
  src/items.rs    tree CRUD (create/duplicate/delete); moving.rs drag & drop + "move to…"; rename.rs inline rename
  src/backend.rs  bridge to the engine: tokio runtime, load/save/send, Voleeo import
  src/mcp.rs      MCP: engine server on `mcp.sock` + `mcp-token` (data dir), `yuki mcp` stdio bridge; agent edits reload the tree and open tabs (`reload_clean_tabs`: clean tabs rebuild from disk, dirty ones keep their edits, deleted ones close); mcp_view.rs settings → agents (mcp): per-client setup snippet (this binary's path, no token), opened by the `● mcp` chip
  src/update.rs   self-update: `Updater` global; GitHub `releases/latest` on launch (only from a `.app`) or by hand (about) → `↑ vX` chip → swap bundle, relaunch
  src/ws_folder.rs workspace in a user folder: ⌘O → + open folder… (new, or the one already there), settings → move…, and settings → general → workspaces folder (all of them, `<base>/<name>`; `Prefs::workspaces_dir`, new imports too); `workspaces/{id}` becomes a symlink to it
  src/transfer.rs import (yuki_import::plan → seal → stores) / export (open secrets → Bundle → yuki_export); transfer_ui.rs window glue; import_view.rs + import_render.rs, export_view.rs screens. Imports flatten inherited auth/headers onto requests (ADR-20261013)
  src/auth.rs     auth schemes as data (fields per type) + AuthConfig⇄form via serde; auth_form.rs auth pane, generic over `AuthHost` (request editor, scope tab); oauth.rs "get token"; secrets.rs secret crypto
  src/scope_view.rs folder/workspace tab (click a folder; empty-sidebar menu for the workspace): auth · variables · headers · notes (`folder_{id}.md`), autosave; scopes.rs the one scope tab slot on `Yuki` (`scope`, `scope_active`); kv.rs name/value rows (variables, headers)
  src/draft.rs, tree.rs, ui.rs  pure helpers
crates/yuki-*     engine inherited from Voleeo (MIT), no gpui/tauri
  yuki-core types+errors · yuki-storage YAML · yuki-http executor · yuki-mcp ApiBackend+resolve
  yuki-crypto AES-GCM/keychain · yuki-auth/oauth/cookies/ws/grpc/import/export/git
site/             GitHub Pages landing (static HTML/CSS/JS, real app screenshots); `.github/workflows/pages.yml` deploys it on push to main
```

**Invariants:** `crates/` never depends on gpui. The whole send pipeline goes through `ApiBackend::send_http` (`crates/yuki-mcp/src/api/request/send.rs`) — app and MCP share it; never reimplement env/folder/auth/cookies/history in the UI. Variable resolution: `yuki_mcp::resolve` (`load_env_vars_from`, `apply_folder_vars`, `resolve_str`, `is_identifier`); `resolve_str` also evaluates no-arg template functions from `resolve::funcs::FUNCTIONS` (`{{ $fake.email() }}` — `$` sigil required, fresh per occurrence; unknown/with-args stay verbatim). `send_http` applies the workspace's DNS overrides, shell-env allowlist and certificate-check skip, merges workspace → folder → request headers (`merge_inherited_metadata`) and resolves `inherit` auth (`effective_auth`: nearest folder with an active auth, else the workspace) for HTTP; WS/gRPC still treat `inherit` as no auth.

## Localization

- Supported: `en` (source) and `pt-BR`. Code, comments, `AGENTS.md`, skills and commits are English. UI copy is English inside `t!("...")` (rust-i18n, gettext style): the English text **is** the key; every key goes into every `app/locales/<lang>.yml`.
- User-facing docs are bilingual: `README.md` ↔ `README.pt-BR.md`, `docs/legal/TERMS.md` ↔ `TERMS.pt-BR.md` — change both in the same commit.
- Any copy or doc change: skill `yuki-i18n` (rules, adding a language). Validate with `cargo test -p yuki locales_test` (missing, stale and divergent keys) and `.agents/skills/yuki-i18n/scripts/doc-parity.sh`.
- Language: `YUKI_LANG` (dev override) > settings → appearance (`settings.yaml` `language`) > system locale (`sys-locale`). Switching is live, but text cached at creation (input placeholders, select lists) only changes after a restart.

## Data

- Directory: `~/Library/Application Support/dev.yuki` (override `YUKI_DATA_DIR`). Same layout as Voleeo (`workspaces/{id}/req_*.yaml`, `envs-local/{id}/`, `keys/{id}.key`, `responses-local/`, machine-local `workspace-settings.yaml` — edit as a YAML tree, Voleeo keys ride along). App prefs: `settings.yaml`; UI session: `session.yaml`.
- Import: `backend::import_voleeo` **copies** (never links) workspaces, local envs and key files; skips `.git` and existing ids.
- Local-first: edits hit the disk 400 ms after the last keystroke (`editor::refresh_dirty` → `save`; one write at a time). ⌘S, send, closing a tab and quitting only bring it forward. Saving writes only if changed (`save_request_if_changed`); header ids are preserved by position (`draft::text_to_headers`) — fresh ids = phantom diff in the user's git.
- Encrypted workspace: auth secrets and secret env variables are opened on load, sealed on save (`secrets.rs`). An unchanged secret reuses the stored ciphertext — re-encrypting draws a new nonce = phantom diff.
- Request variations (`variations` + `activeVariation` in `req_{id}.yaml`, ADR-20261015): independent copies of url/params/headers/body/auth (a missing field falls back to the base). `HttpRequest::effective`/`shown` is the only overlay — `send_http` applies it after loading, so history and snapshots never hold variations; secret walkers use `auths_mut()`.
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

CI (`.github/workflows/ci.yml`, macOS) runs fmt `--check`, clippy `--workspace --all-targets -D warnings` and `cargo test --workspace`. Release: Actions → `release` → Run workflow on `main`, no input — git-cliff (`cliff.toml`) bumps from the `feat`/`fix`/`perf` commits since the last tag (none = no release), each arch builds on its own runner into a draft (`Yuki-macos-{aarch64,x86_64}.{dmg,zip}`; the zip feeds the updater), then main gets the bump commit + tag (deploy key `RELEASE_DEPLOY_KEY`, the only bypass of the `main` ruleset besides admins) and the release is **published** — the updater ships it right away.

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
8. **Template chips:** `vars::content` on load and `chip_templates!` on every `InputEvent::Change`; saved text stays literal (`{{ BASE }}`). Inputs take the renderer via `var_view::chips`; code editors (body) via `var_view::pin_chips` on every render — stock gpui-kit rejects tokens in `EditorState` and its `Editor` has no token builder, so `vendor/gpui-base` patches both (changes marked `yuki:`, wired by `[patch.crates-io]`; keep it when bumping gpui-kit until upstream has them).
9. **Popup keys over `Input`:** `Input` consumes ↑↓↵⇥esc. To win, bind with context `"VarComplete > Input"` (same depth, registered later) and `key_context("VarComplete")` on the ancestor only while the popup is open. ⌘space belongs to Spotlight — it never reaches the app.
10. **Single-line cells:** `InputState` strips line breaks; `draft::cell_value` returns the saved value if the cell wasn't touched (avoids corrupting multi-line values).
11. **Theme-driven prefs:** mutate `Theme::global_mut(cx)` then `cx.refresh_windows()` (`prefs.rs`). Root sets the rem size from `theme.font_size` every frame, so text scales but `px(..)` row heights don't — the UI font is capped at 16. `mono_font_size` drives code editors and table cells.
12. **Icons:** `component::IconName` covers only gpui-kit's default bundle. Any other Lucide icon (`assets::IconName::X` via `Icon::new`) renders **blank, no error**, until it's added to `icon_assets!(ExtraIcons, [...])` in `main.rs`. Never `AllAssets` (embeds the 7 MB catalog).

## Delivery flow

1. Read the area + relevant skill. 2. Implement with the smallest diff. 3. `cargo build -p yuki`, `clippy`, tests. 4. **Run the app** and check the changed surface (skill `yuki-visual-smoke`) — compiling is not proof. 5. Clean up temporary fixtures/scripts.

Commits: Conventional Commits, in English, no AI co-authorship. Never commit/push without being asked.
