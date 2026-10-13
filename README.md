# Yuki

**English** · [Português](README.pt-BR.md)

A native, keyboard-first API console for macOS, written in Rust on GPUI. One document per request (`GET ▸ url`), monospace wherever there's data, color only where it means something.

Pre-release: there's no packaged `.app` yet; run it from source.

## Run

```bash
cargo run -p yuki                        # dev
YUKI_DATA_DIR=/tmp/yk cargo run -p yuki  # isolated data dir
YUKI_LANG=pt-BR cargo run -p yuki        # force a UI language (default: system)
cargo build --release -p yuki
```

## Keys

| Key | Action |
|---|---|
| ⌘↵ | send request |
| ⌘P / ⌘⇧P / ⌘O | go to request / commands / switch workspace |
| ⌘N / ⌘⇧N | new request / new folder |
| ⌘E | toggle the request editor |
| ⌘⇧E | variables panel |
| ⌘⌥E | environments and variables |
| ⌘⇧G | git panel |
| ⌘, | settings: workspace name, DNS overrides, shell env vars, font sizes |
| ⌘B | toggle sidebar |
| ⌘W / ⌃⇥ / ⌃⇧⇥ | close / next / previous tab |
| ⌘S | save now (edits already autosave) |

## Data

Everything lives in `~/Library/Application Support/dev.yuki` (override with `YUKI_DATA_DIR`) as plain YAML: one folder per workspace, each its own git repository. Voleeo workspaces can be imported (copied, never linked) from the command palette.

## Contributing

Issues and pull requests are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md). Security problems go through [SECURITY.md](SECURITY.md), never a public issue.

## License

MIT — see [LICENSE](LICENSE) and [NOTICE](NOTICE). The engine crates (`crates/yuki-*`) derive from Voleeo. The code is fully open; commercial use of the official binaries is covered by [docs/legal/TERMS.md](docs/legal/TERMS.md).
