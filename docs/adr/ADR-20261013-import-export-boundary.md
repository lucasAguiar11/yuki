---
title: "ADR-20261013-import-export-boundary: Pure engine crates, app owns secrets and persistence; inheritance flattened on import"
status: "Accepted"
date: "2026-10-13"
authors: "user + agent"
feature_id: FEAT-20261013-import-export
scope: "import-export"
tags: ["import", "export", "secrets", "auth-inheritance", "file-formats"]
supersedes: ""
superseded_by: ""
---

# ADR-20261013-import-export-boundary

## Status

Accepted

## Context

`yuki-import` (parsers → IR → core) and `yuki-export` (core → Voleeo bundle /
Postman) came from Voleeo without the Tauri layer that persisted and decrypted.
Yuki's send pipeline ignores `AuthConfig::Inherit` and folder/workspace headers
(AGENTS.md), while Postman/Insomnia/Yaak put auth and headers on collections and
folders. External formats only hold plaintext secrets.

## Decision

1. The crates stay pure (no storage, no crypto, no gpui). `yuki-import` exposes
   one `plan(...)` entry returning core types ready to store; `yuki-export` takes
   already-decrypted `Bundle`s. The app (`app/src/transfer/`) reads/writes files,
   opens/seals secrets via `secrets.rs` and persists through the stores.
2. Import **flattens** inheritance: each request gets the effective auth and the
   merged headers (workspace → folders → request; nearer wins on name) of its
   source collection; imported folders/workspace carry no auth/headers.
3. Export asks **include secrets** (default off). Off: `Bundle::strip_secrets`
   blanks every secret value before serializing.
4. Insomnia: import v4 JSON and v5 YAML; export v4 JSON. Yaak export targets
   `yaakSchema: 4` (the shape of the real export fixture the parser reads).

## Consequences

### Positive

- POS-001: Imported requests send with their auth today, with no pipeline change.
- POS-002: Exports are safe to share by default.
- POS-003: Engine logic is testable as round-trips (export → import) without the app.

### Negative

- NEG-001: Editing a shared token after import means editing every request; once the pipeline resolves inheritance, flattening becomes a loss (revisit then).
- NEG-002: Yuki → Voleeo bundle → Yuki also flattens (one rule for every format).
- NEG-003: Exports without secrets need values re-entered on the other side.

## Alternatives Considered

### Resolve inheritance in `send_http`

- ALT-001: Correct and reusable, but a separate, larger change to the shared send pipeline (app + MCP). Rejected for now by the user.

### Always export secrets / never export secrets

- ALT-002: Rejected: per-export choice, safe default.

## References

- REF-001: docs/features/FEAT-20261013-import-export.md
- REF-002: Insomnia v5 schema https://raw.githubusercontent.com/Kong/insomnia/develop/schemas/insomnia.schema.5.1.json
