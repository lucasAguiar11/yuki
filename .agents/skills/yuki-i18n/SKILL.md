---
name: yuki-i18n
description: Use when adding, changing or removing Yuki UI copy (`t!`), editing README/TERMS or any doc with a translated sibling, or adding a supported language — keeps every locale and doc translation in sync and validated.
---

# Yuki i18n

English is the source language. Every other supported language must be complete: a missing key silently shows English, a stale key is dead weight that hides real gaps.

## Supported languages

| Code | UI copy | Docs |
|---|---|---|
| `en` | source text inside `t!("…")` (no file) | `README.md`, `docs/legal/TERMS.md` |
| `pt-BR` | `app/locales/pt-BR.yml` | `README.pt-BR.md`, `docs/legal/TERMS.pt-BR.md` |

English only (agent-facing): `AGENTS.md`, `.agents/skills/**`, code comments, commits.

## Changing UI copy

1. Write English, lowercase, short, inside `t!("...")`. Placeholders: `t!("%{n} min ago", n = mins)` — never `format!` around copy. No plurals: separate keys (`git_panel::plural`).
2. Add the same key to **every** `app/locales/<lang>.yml`: `"English": "translation"`, both quoted, even when identical.
3. Changed English text = changed key: rename it in every locale file (the old one is now stale).
4. Removed copy: delete its key everywhere.
5. Keys passed as variables (`t!(label)` over a table of `&'static str`) still need their literal somewhere in `src/` — the tests look for it.
6. Not copy (never `t!`): methods, header names, `{{ VAR }}`, scheme names, serde values, element ids, `eprintln!`/`expect` messages.

## Changing a doc

Edit the English file and every `X.<lang>.md` sibling in the same change: same sections, tables, code blocks, links. Language links at the top (`**English** · [Português](README.pt-BR.md)`). Legal text (`TERMS*`): translate faithfully, never paraphrase terms.

## Validate (both must pass before done)

```bash
cargo test -p yuki locales_test             # UI: app/src/locales_test.rs
.agents/skills/yuki-i18n/scripts/doc-parity.sh   # docs: same shape per translation
```

| Test | Catches |
|---|---|
| `every_locale_covers_every_literal_key` | `t!("…")` literal missing in a locale |
| `no_stale_keys` | locale key no longer a string literal in `src/` (changed/removed English) |
| `locales_share_one_key_set` | locales disagree (covers variable keys) |
| `doc-parity.sh` | translated doc with different headings / table rows / code fences / links, or orphan translation |

Then look at the UI in each language: `YUKI_LANG=en` and `YUKI_LANG=pt-BR` (skill `yuki-visual-smoke`) — longer translations overflow fixed-width labels.

## Adding a language

1. `app/locales/<lang>.yml` with every key of `pt-BR.yml` (file stem = locale code, e.g. `es`, `fr-FR`).
2. `README.<lang>.md` + `docs/legal/TERMS.<lang>.md`, and add the language link to every README variant.
3. Add a row to the table above and run the validation.

## Gotchas

- The key scanner reads `src/*.rs` text: a comment or doc string containing `t!("x")` becomes a required key. Write `t!(…)` in prose.
- `no_stale_keys` matches the key's Rust-escaped form (`"a \"b\""`); a key split across lines with `\` continuation reads as stale — keep keys on one line.
- `app/build.rs` makes cargo rebuild when `locales/` changes; translations are embedded at compile time, so rebuild before a smoke run.
