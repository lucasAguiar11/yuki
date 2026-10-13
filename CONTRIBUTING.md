# Contributing to Yuki

Yuki is pre-release and small: issues and focused pull requests are welcome.

## Before you start

- Bugs and small fixes: open a PR directly.
- Features and UI changes: open an issue first. Yuki has a deliberate visual direction (see [AGENTS.md](AGENTS.md), "Product") and not every feature fits.
- Security problems: never in a public issue, see [SECURITY.md](SECURITY.md).

## Build

macOS only for now, Rust stable.

```bash
cargo run -p yuki                         # dev
YUKI_DATA_DIR=/tmp/yk cargo run -p yuki   # test with isolated data, never your real workspaces
```

## Checks

CI runs these on every PR:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Compiling proves nothing about UI: for a UI change, run the app and put a screenshot in the PR.

Merging into `main` needs green CI and the maintainer's review (`.github/CODEOWNERS`); PRs are squash-merged, so the PR title becomes the commit message and should follow the convention below.

## Conventions

[AGENTS.md](AGENTS.md) is the project guide, for people and coding agents alike: layout, invariants, GPUI pitfalls. The short version:

- `crates/` never depends on gpui; every send goes through `ApiBackend::send_http`.
- Colors only from the theme (`app/themes/yuki-console.json`), never hex in code.
- UI copy is English, lowercase and short, inside `t!("...")`; every new key also goes into `app/locales/pt-BR.yml` (`cargo test -p yuki locales_test` checks it).
- `README.md` and `docs/legal/TERMS.md` have pt-BR twins: change both (`.agents/skills/yuki-i18n/scripts/doc-parity.sh` checks it).
- Workspace, request and environment YAML on disk belong to the user: keep it readable, no breaking format changes.
- New dependencies: nothing GPL, LGPL or AGPL.
- Commits follow [Conventional Commits](https://www.conventionalcommits.org/) (`feat(app): …`, `fix(storage): …`).

## License

Contributions are licensed under the [MIT License](LICENSE), like the rest of the code. The official prebuilt binaries are offered for commercial use under separate [terms](docs/legal/TERMS.md); MIT already allows that, so there is no CLA.
