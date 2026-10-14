---
feature_id: FEAT-20261013-import-export
status: done
owner: TBD
depends_on: [FEAT-20261009-request-notes]
impacts: [yuki-import, yuki-export, workspaces, environments, secrets]
adr: [docs/adr/ADR-20261013-import-export-boundary.md]
plan: docs/plans/FEAT-20261013-import-export-plan.md
---

# FEAT-20261013-import-export - Import/export Voleeo, Yaak, Postman and Insomnia

## Objective

Bring collections in from Voleeo, Yaak, Postman and Insomnia files, and take a
workspace out to the same four formats, from inside the app.

## Scope

- **Import** (⌘⇧P "Import collection…", welcome screen, ⌘O): pick a file; the
  format is detected (Voleeo bundle, Yaak schemas 1–5, Postman v2.0/2.1
  collections and environment/globals files, Insomnia v4 JSON and v5 YAML
  collections, design documents and environment files; OpenAPI/Swagger/Bruno
  come along since the engine has them). Variables-only files (environments)
  import with no request selected.
- Auth maps to every scheme Yuki has (basic, bearer, API key, OAuth1, OAuth2
  with all its options, AWS SigV4, Digest, NTLM), following each source's
  inheritance rules (absent = inherit, explicit none = no auth); a custom
  bearer prefix or a cookie API key becomes a header. Schemes Yuki lacks
  (hawk, JWT, ASAP, …), scripts, dynamic variables and skipped WebSocket/gRPC
  items each produce one warning. File paths (multipart, binary), disabled
  variables and Yaak folder environments are kept.
- Import screen: tree preview with checkboxes (all checked), counts, warnings;
  destination **new workspace** (named after the collection) or **current
  workspace** (root or a chosen folder).
- Inherited auth and folder/collection headers are **flattened** onto each
  imported request (the send pipeline ignores `inherit` and folder headers).
- Environments come along: globals into the Global env, named ones as local
  environments; Voleeo bundles keep their WebSocket/gRPC items and `shared` flags.
- Importing into an encrypted workspace seals secrets like a normal save.
- **Export** (⌘⇧P "Export workspace…"): choose format (Voleeo, Yaak, Postman,
  Insomnia v4 JSON) and **include secrets** (off by default — secret values are
  written empty); save dialog; Postman environments go to sibling
  `*.postman_environment.json` files.
- The old "import workspaces from Voleeo" (copy of Voleeo's data dir) stays.

## Non-Goals

- Export as Insomnia v5, OpenAPI or Bruno; Postman Collection v1 import.
- Implementing auth/header inheritance in the send pipeline.
- Creating encrypted workspaces from the import.
- Request notes (`req_*.md`) in exports; scripts, cookies, tests.

## Contracts

- Inputs: a collection file (JSON/YAML) chosen by the user.
- Outputs: workspace/folder/request/env YAML through the existing stores; an
  export file (plus Postman env files) at the user-chosen path.
- Existing modules read: `yuki-core` types, `yuki-storage` stores, `app/src/secrets.rs`.
- Existing modules changed: `yuki-import` (Voleeo bundle, Insomnia v5, named
  envs, flattening), `yuki-export` (Yaak, Insomnia v4, secret stripping), app
  (import screen, export dialog, palette/welcome entries).

## Acceptance Criteria

- [x] Each of the four formats exported by Yuki re-imports into Yuki with the same folders, requests (method, url, headers, params, body, auth) and environment variables.
- [x] Real-world fixtures (Postman 2.1, Insomnia v4, Insomnia v5, Yaak) import through the app with preview, selection and both destinations.
- [x] An imported request whose collection/folder carried auth or headers carries them itself (flattened; verified on stored data, a live send was not exercised).
- [x] Export without "include secrets" has no secret value in the file; with it, values are plaintext.
- [x] Importing into an encrypted workspace stores secrets as `enc:v1:…`.
- [x] Unsupported or lossy items show up as warnings, never silently.

## Blocking Decisions

Resolved by user on 2026-10-13: secrets asked at export time (off by default);
flatten inheritance on import; destination new or current workspace; preview
with selection; Insomnia: read v4+v5, export v4. See ADR.

## Assumptions

- A bundle/collection with several workspaces imports as one top-level folder per workspace.
- Imported named environments are local (`shared: false`) except Voleeo bundle envs, which keep their flag.
- Ids are regenerated on import (no collision with existing files).

## Impact

- Allowed write paths: `crates/yuki-import/**`, `crates/yuki-export/**`, `app/src/**`, `app/Cargo.toml`, `app/locales/pt-BR.yml`, `README*.md`, `AGENTS.md`, `Cargo.lock`.
- Read-only paths: `crates/yuki-core/**`, `crates/yuki-storage/**`, `crates/yuki-crypto/**`, `crates/yuki-mcp/**`.

## Open Questions

- None.
