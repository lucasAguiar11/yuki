---
name: yuki-visual-smoke
description: Use to verify Yuki UI changes by actually running the app on macOS — isolated fixture data, driving keys/mouse, capturing and comparing screenshots, measuring drag/resize behavior — before claiming a UI change works.
---

# Yuki visual smoke

Compiling proves nothing about UI. Run the real app on isolated data, drive it, look at screenshots.

## Safety first

- **The user often has their own Yuki open.** Always target your process by **PID** (`wid <pid>`), never by app name. Driving their window has already happened twice. Both windows open at the **same screen position**, so a click/keystroke can land on theirs even with yours "frontmost" — move yours first: `osascript -e 'tell application "System Events" to set position of first window of (first process whose unix id is <pid>) to {700, 120}'`, then re-read `wid`.
- `cargo clippy`/`cargo test` do not refresh `target/debug/yuki`: run `cargo build -p yuki` before launching.
- Keep the window inside the main display (x ≤ ~800 for a 1280-wide window on a 1920 screen): synthetic clicks past the screen edge are silently dropped — looks like a dead button.
- Keystrokes/clicks go to the real machine: keep sessions short, set your app frontmost right before each burst, and tell the user if they may be typing.
- Never use the real data dir (`~/Library/Application Support/dev.yuki`) — always `YUKI_DATA_DIR=/tmp/...`.
- Never send requests to the user's real APIs; point fixtures at a local server.

## Setup

```bash
S=.agents/skills/yuki-visual-smoke/scripts
swiftc -O $S/wid.swift -o /tmp/yk-wid && swiftc -O $S/mouse.swift -o /tmp/yk-mouse
$S/fixture.sh /tmp/yk                      # default: Voleeo "demo" workspace (ids are machine-specific)
python3 -m http.server 8765 --bind 127.0.0.1 --directory /private/tmp/yk/srv   # as a background service
cargo build -p yuki && YUKI_DATA_DIR=/tmp/yk ./target/debug/yuki               # as a named background service
```

Get the PID from the service, then `read W X Y <<< "$(/tmp/yk-wid <pid>)"` (window number + screen origin, 1× points).

## Drive

```bash
osascript -e 'tell application "System Events" to set frontmost of (first process whose unix id is <pid>) to true'
osascript -e 'tell application "System Events" to keystroke "p" using {command down}'   # ⌘P etc.; key code 36 = ↵, 53 = esc
/tmp/yk-mouse click $((X+105)) $((Y+123))      # window-relative point + origin
/tmp/yk-mouse move  $((X+390)) $((Y+89))       # hover (tooltips need ~1 s)
/tmp/yk-mouse drag  $((X+240)) $((Y+400)) $((X+520)) 8 $W /tmp/dr/f   # frames f1..f8.png
/tmp/yk-mouse rclick $((X+80)) $((Y+80))       # context menu
/tmp/yk-mouse cmdclick $((X+566)) $((Y+89))    # ⌘-click (chip → origin); no global ⌘ held
/tmp/yk-mouse dragto $((X+90)) $((Y+200)) $((X+60)) $((Y+176))   # drag & drop (sidebar rows)
```

- The user's own Yuki often overlaps yours and the terminal steals focus back: guard every burst with `[ "$(osascript -e 'tell application "System Events" to get unix id of first process whose frontmost is true')" = "$P" ] && …` so a click/key never lands in their window.
- A click on an inactive window only activates it — set frontmost first, **in the same command** as the keystrokes: focus drifts back to the terminal between tool calls and the keys silently go nowhere.
- Option combos: `keystroke "e" using {command down, option down}` types a dead key and never matches `cmd-alt-e` — use `key code 14 using {…}`.
- Elements move when font size or layout changes (e.g. after a `+` that grows text): re-capture before clicking the same spot again.
- Clipboard checks: `pbpaste`.
- Logs: the app prints `[send] …` lines; read the service output.

## Capture & inspect

```bash
screencapture -x -o -l$W /tmp/s1.png
```

- **Stale frames:** a capture right after input may show the previous frame — capture twice with a short sleep and use the second.
- **Read-tool lag:** freshly written PNGs may report "not found" for ~10 s; `sleep 10` then read via `/private/tmp/...`.
- Compare states in one image (PIL is installed): crop/resize several captures into a grid and read that once.
- Resize/drag bugs: drag in steps with per-step frames and check the divider follows the mouse monotonically (measure columns with PIL if needed).

## Finish

Kill your services (only yours), `rm -rf /tmp/yk /tmp/yk-* /tmp/*.png` you created, and report exactly what was exercised and what was not (e.g. "hover verified via tooltip; drag verified 240→520→300").
