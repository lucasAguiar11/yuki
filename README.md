<img src="app/assets/logo.svg" width="96" alt="Yuki">

# Yuki

**English** · [Português](README.pt-BR.md)

A native, keyboard-first API console for macOS, written in Rust on GPUI: an open-source Postman alternative. One document per request (`GET ▸ url`), monospace wherever there's data, color only where it means something. Website: [lucasaguiar11.github.io/yuki](https://lucasaguiar11.github.io/yuki/).

Download `Yuki-macos-aarch64.dmg` (Apple Silicon) or `Yuki-macos-x86_64.dmg` (Intel) from the [latest release](https://github.com/lucasAguiar11/yuki/releases/latest) and drag Yuki to Applications (it updates itself afterwards), or run it from source.

**First launch.** Yuki isn't signed with an Apple Developer certificate or notarized yet, so macOS refuses to open it the first time ("Apple could not verify…"). Allow it once by hand: try to open it, then System Settings → Privacy & Security → **Open Anyway**. Or from a terminal:

```bash
xattr -dr com.apple.quarantine /Applications/Yuki.app
```

Updates installed by Yuki itself don't need this step.

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
| ⌘⇧O | open a workspace in a new window (or ⌘-click / right-click one on the welcome screen); a workspace open in another window is brought forward |
| ⌘N / ⌘⇧N | new request / new folder |
| ⌘E | toggle the request editor |
| ⌘⌥L | response beside / below the request |
| ⌘⇧E | variables panel |
| ⌘⌥E | environments and variables |
| ⌘⇧G | git panel |
| ⌘, | settings: workspace name, DNS overrides, shell env vars, font sizes |
| ⌘B | toggle sidebar |
| ⌘W / ⌃⇥ / ⌃⇧⇥ | close / next / previous tab |
| ⌘S | save now (edits already autosave) |

## Data

Everything lives in `~/Library/Application Support/dev.yuki` (override with `YUKI_DATA_DIR`) as plain YAML: one folder per workspace, each its own git repository. A workspace can also live in a folder of yours (a repo, a project folder): ⌘O → **+ open folder…** creates a new workspace in an empty folder or opens the one already there (a clone, say); Yuki only keeps a link to it. Settings → workspace shows where it lives and **move…** takes it, `.git` included, to another empty folder. Settings → general → **workspaces folder** puts them all in one folder of yours (a subfolder each, named after the workspace); new imports land there too. Voleeo workspaces can be imported (copied, never linked) from the command palette.

## Variables and functions

`{{ NAME }}` in the URL, params, headers, body or auth resolves from the active environment (and folder variables) at send time. Functions start with `$` and generate a fresh value per occurrence: `{{ $fake.fullName() }}`, `firstName`, `lastName`, `email`, `phone`, `cpf`, `cnpj`, `uuid`, `lorem`, `number`, `password`, `date`, `username`, `company`, `street`, `city`, `zipCode`, `jobTitle`, `url` (all `$fake.*`), plus `{{ $uuid.v4() }}` and `{{ $timestamp.iso() }}` / `unix` / `unixMs` / `date` / `time`. Type `$` to pick one (the list shows a sample of each); the timeline shows the values that were sent.

**Values from a response.** Click a value in a JSON response (say `access_token` of a login) and press `→ variable` (⌥↵): name it and the active environment gets `TOKEN = {{ $response.body(requestId="…", selector="$.data.access_token") }}`. It's a pointer: sending any request that uses `{{ TOKEN }}` sends the login first and takes the field from that fresh response, so the token is always current (a source that would loop back is read from its last response instead). The bar under the response lists the variables that read it (✕ removes one).

No variable needed: in any template field — a folder's or the workspace's auth included — type `$response.` and the autocomplete lists the fields of each request's latest JSON response (`login › $.accessToken`); keep typing to narrow it, `$response.login.token` (request name, then field). Plain `$` offers `$response.…` at the top, and `$token` alone also finds matching fields. Picking one inserts the same `$response.body(…)` pointer.

**Folders and the workspace.** Click a folder to open it as a tab: auth, variables, headers and notes for every request inside it and its subfolders (the chevron only expands/collapses). A request with auth `inherit` sends the nearest folder's auth, else the workspace's; headers stack workspace → folders → request, the nearer one winning on the same name. Right-click the empty sidebar → **workspace auth & headers…** edits the workspace level.

**Variations.** One request, several ways to send it — say `admin` and `user` tokens — instead of duplicates. The `base ▾` chip left of the request tabs lists them: **new variation…** copies what's on screen (URL, params, headers, body, auth) into a new variation and makes it active. Each variation is independent: editing one never changes the base or the others, and editing the base never changes them. Pick a variation to send it. The active variation is saved in the request file, so the team shares it. Over MCP, `request.send` takes `variation` (name or id). Exporting to Postman, Insomnia or Yaak turns each variation into its own request right after the base (`me · admin`); a Voleeo bundle keeps them as variations.

**Comments in JSON bodies.** `// …` and `/* … */` in a JSON body stay in the request but are removed when it's sent (a comma left before `}` or `]` too), so you can comment out a field.

## Import / export

⌘⇧P → **Import collection…** (or ⌘O → **+ import collection…**) reads a Voleeo bundle, Yaak, Postman (v2.0/2.1), Insomnia (v4 JSON or v5 YAML), OpenAPI/Swagger or Bruno file: pick what comes in from the tree, into a new workspace or into a folder of the open one. Auth and headers set on the collection or a folder are copied onto each request. **Export workspace…** writes the open workspace as Voleeo, Yaak, Postman (environments as sibling files) or Insomnia v4; secret values are left empty unless you tick **include secrets**.

A `curl …` command pasted into the URL bar (DevTools **Copy as cURL**, API docs) becomes the open request: method, URL and query, headers, body and basic auth.

## MCP

While Yuki is open, AI agents can list, create, edit and send your requests (HTTP, WebSocket, gRPC), environments, cookies, history and snapshots over MCP. The client launches `yuki mcp`, a stdio bridge to the running app — no token or port to configure.

**1. Find the binary.** Installed app: `/Applications/Yuki.app/Contents/MacOS/yuki`. From source: `target/release/yuki` (after `cargo build --release -p yuki`). Use the absolute path.

**2. Add it to your client** — easiest from the app: click `● mcp` in the status line (or settings → agents (mcp)), pick the client and copy the snippet, already filled with this binary's path. By hand (built from source? swap in your path):

| Client | Setup |
|---|---|
| Claude Code | `claude mcp add -s user yuki -- /Applications/Yuki.app/Contents/MacOS/yuki mcp` |
| Codex | `codex mcp add yuki -- /Applications/Yuki.app/Contents/MacOS/yuki mcp` |
| Cursor | `~/.cursor/mcp.json` (or `.cursor/mcp.json` in a project), JSON below |
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json`, JSON below; restart it |

```json
{ "mcpServers": { "yuki": { "command": "/Applications/Yuki.app/Contents/MacOS/yuki", "args": ["mcp"] } } }
```

**3. Keep Yuki open.** `● mcp` in the status line is green while the server is up (with the number of connected agents) and red if it couldn't start. Ask the agent to "list my Yuki workspaces" to check.

- `open Yuki once` — the app never ran with this data dir: open it, then retry.
- `is Yuki running? no socket` — the app is closed: open it; the client reconnects on its next start.
- Running Yuki with `YUKI_DATA_DIR`? Give the bridge the same value: `"env": { "YUKI_DATA_DIR": "/tmp/yk" }` in the JSON, `-e YUKI_DATA_DIR=…` (Claude Code) or `--env YUKI_DATA_DIR=…` (Codex).

Agent edits show up in the sidebar at once; a request already open in a tab keeps what it had until you reopen it.

## Contributing

Issues and pull requests are welcome: see [CONTRIBUTING.md](CONTRIBUTING.md). Security problems go through [SECURITY.md](SECURITY.md), never a public issue.

## License

MIT — see [LICENSE](LICENSE) and [NOTICE](NOTICE). The engine crates (`crates/yuki-*`) derive from Voleeo. The code is fully open; commercial use of the official binaries is covered by [docs/legal/TERMS.md](docs/legal/TERMS.md).
