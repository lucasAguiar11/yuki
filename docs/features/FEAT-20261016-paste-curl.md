---
feature_id: FEAT-20261016-paste-curl
status: done
owner: TBD
depends_on: []
impacts: [request-editor]
adr: []
plan: docs/plans/FEAT-20261016-paste-curl-plan.md
---

# FEAT-20261016-paste-curl - Paste a cURL command into the URL bar

## Objective

Pasting `curl …` (DevTools "Copy as cURL", API docs) into the URL bar turns the
open request into that command: method, URL + query, headers, body, basic auth.
The reverse of "copy as cURL".

## Scope

- Trigger: a paste into the URL bar whose whole value starts with `curl `.
- Parsed: `-X/--request`, URL (positional or `--url`), `-H/--header`,
  `-d/--data/--data-raw/--data-ascii/--data-binary` (JSON body when it parses
  as JSON or Content-Type says so, else text), `--data-urlencode` (form),
  `-F/--form/--form-string` (multipart, `@path` = file), `-u/--user` (basic),
  `-G/--get`. Unknown flags (`-L`, `--compressed`, `-k`, …) are skipped.
- Applied to what the editor shows (the active variation or the base), through
  `RequestEditor::change`, so it saves like any edit.

## Non-Goals

- A "new request from cURL" command, cURL files, `@file` data bodies.

## Acceptance Criteria

- [x] Pasting the output of "copy as cURL" rebuilds an equivalent request.
- [x] A Chrome DevTools cURL (multi-line, `-H` + `--data-raw`) becomes POST with headers and JSON body; the query lands in params.
- [x] Pasting a plain URL keeps today's behavior.

## Blocking Decisions

None (no persistence/contract change).

## Impact

- Allowed write paths: `app/src/{paste_curl,editor,main}.rs`, `app/Cargo.toml`, `Cargo.lock`, `AGENTS.md`, `README.md`, `README.pt-BR.md`, docs.
