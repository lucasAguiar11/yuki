---
feature_id: FEAT-20261015-request-variations
status: done
owner: TBD
depends_on: []
impacts: [request-yaml, send-pipeline, mcp-send, secrets, request-editor, import-export]
adr: [ADR-20261015-request-variations-inline]
plan: docs/plans/FEAT-20261015-request-variations-plan.md
---

# FEAT-20261015-request-variations - Request variations

## Objective

One request, several named variations (e.g. `admin`, `user`, `expired token`)
instead of duplicating the request per credential. The user picks which
variation runs; the base stays the single source for everything a variation
doesn't override.

## Scope

- A variation is `{id, name}` plus an **independent copy** of `url`,
  `parameters`, `headers`, `body`, `auth` (revised by user 2026-10-15: editing
  one never changes the base or the others; editing the base never changes the
  variations). A field missing on disk (older file) falls back to the base's
  until the variation is next saved.
- The active variation is stored in the request YAML (`activeVariation`); none
  = base.
- Send (app and MCP) applies the active variation — or an explicit one passed to
  MCP `request.send` (`variation`: id or name) — over the base, once, inside
  `ApiBackend::send_http`. History, timeline and snapshots hold only the
  effective request.
- Editor: a variation chip leftmost in the meta line (`base ▾`) opens a menu:
  base and each variation (pick to switch), `new variation…` (copies what's on
  screen), and for the active one rename and delete (confirmed). With a
  variation active the editor shows it and every edit is saved into it. The
  chip is disabled while a send or save is in flight.
- Duplicate copies variations; Voleeo-bundle export/import carries them;
  Postman/Insomnia/Yaak export turns each into its own request right after the
  base (`{request} · {variation}`, revised by user 2026-10-15); secrets in
  variation auth are sealed/opened/masked like the base auth.

## Non-Goals

- Variations for folders, WebSocket or gRPC.
- Overriding `method` or `name`.
- Editing variations through MCP (`request.update` edits the base only;
  `request.get`/`list` show them).

## Contracts

- On disk (`req_{id}.yaml`): `variations` (omitted when empty) and
  `activeVariation` (omitted when none). Files without variations re-serialize
  byte-identically.
- MCP `request.send`: optional `variation` (id or name); unknown → error.

## Acceptance Criteria

- [x] Request `GET /me` with base auth `TOKEN_A`; new variation `admin` with
      auth `TOKEN_B`; ⌘↵ with `admin` active sends `TOKEN_B`; switching to base
      sends `TOKEN_A`; the base auth on disk never changed.
- [x] Switching variation persists `activeVariation` and survives restart.
- [x] `use base` on `auth` in `admin` → sends `TOKEN_A`; the override is gone
      from the YAML.
- [x] MCP `request.send` with `variation: "admin"` sends the admin auth
      regardless of the active one; `request.get` masks variation secrets.
- [x] Encrypted workspace: variation auth secret is ciphertext on disk; an
      unchanged secret keeps its ciphertext (no phantom diff) — unit-tested
      (`secrets::seal_request`), not exercised end-to-end in the app.
- [x] A request YAML without variations is byte-identical after open + save.

## Blocking Decisions

- What a variation overrides — decided by user 2026-10-15: whole fields (not
  variables only). Variables-only (variation = named `{{ VAR }}` set) was the
  recommended, smaller option.
- Where the selection lives — decided by user 2026-10-15: in the request YAML
  (shared via git; every switch is a file change). Session-local was
  recommended.
- Scope — decided by user 2026-10-15: per request only.

## Impact

- Allowed write paths: `crates/yuki-core/src/types/http.rs`,
  `crates/yuki-storage/src/{request/**,folders.rs}` and storage test literals,
  `crates/yuki-mcp/src/api/{request/send.rs,request/crud.rs,redact.rs,tools/request.rs,server.rs,mod.rs}`,
  `crates/yuki-mcp/src/resolve/http.rs`, `crates/yuki-import/src/{convert,voleeo}.rs`,
  `crates/yuki-export/src/{lib,voleeo}.rs`, test struct literals in
  `crates/yuki-http/**`, `crates/yuki-export/tests/**`,
  `app/src/{editor,document,draft,backend,secrets,transfer,curl,oauth,items,workspace,variation}.rs`,
  `app/locales/pt-BR.yml`, `README*.md`, `AGENTS.md`, docs.
- Read-only: `crates/yuki-storage/src/{response,snapshot}.rs` (consume
  `resolved_request`, already effective).
