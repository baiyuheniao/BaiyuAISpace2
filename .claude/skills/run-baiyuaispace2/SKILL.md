---
name: run-baiyuaispace2
description: Build, run, and drive the BaiyuAISpace2 Tauri desktop app on Windows. Use when asked to start BaiyuAISpace2, build it, take a screenshot of its UI, or interact with its running window (click, navigate, evaluate JS, read state).
---

BaiyuAISpace2 is a Tauri 2 desktop app: Rust backend (`src-tauri/`) + Vue 3
frontend (`src/`), rendered in the OS's native WebView2 (Windows). For
agent/automated use, drive it via
`.claude/skills/run-baiyuaispace2/driver.mjs`, which talks to the running
app's WebView2 over the Chrome DevTools Protocol (CDP) — no Playwright or
other browser-automation dependency needed, just Node's built-in
`fetch`/`WebSocket`/`child_process`.

This is a single-app repo — all paths below are relative to the repo root.

## Prerequisites

Verified present in this environment:

```bash
node -v      # v22.14.0 (Node 22's built-in global WebSocket is what the driver uses)
pnpm -v      # 11.8.0
cargo --version    # cargo 1.96.0
rustc --version    # rustc 1.96.0
```

If `pnpm` is missing: `npm install -g pnpm`. Windows ships WebView2 on
Windows 11 / most Windows 10 installs already — this app's window IS a
WebView2 host, so if WebView2 Runtime isn't present the app won't even
open a window (see Troubleshooting).

## Build

```bash
pnpm install
pnpm build                          # builds the frontend → dist/
cd src-tauri && cargo build         # debug build, faster than --release
```

This produces `src-tauri/target/debug/BaiyuAISpace2.exe`. **Order
matters**: `cargo build` reads `dist/` at compile time
(`frontendDist: "../dist"` in `tauri.conf.json`) — running it before
`pnpm build` has ever populated `dist/` panics with `The 'frontendDist'
configuration is set to "../dist" but this path doesn't exist`.

## Run (agent path)

```bash
node .claude/skills/run-baiyuaispace2/driver.mjs launch
node .claude/skills/run-baiyuaispace2/driver.mjs screenshot out.png
node .claude/skills/run-baiyuaispace2/driver.mjs nav mcp
node .claude/skills/run-baiyuaispace2/driver.mjs click "本地部署"
node .claude/skills/run-baiyuaispace2/driver.mjs eval "document.title"
node .claude/skills/run-baiyuaispace2/driver.mjs quit
```

`launch` spawns the exe **detached** with
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333` and
polls `http://127.0.0.1:9333/json/version` until it responds. Every other
subcommand is a short-lived process that connects fresh to that CDP port,
does one thing, and exits — there's no REPL/tmux to manage, since the app
itself keeps running independently between commands (this environment has
no `tmux`, which is why the driver is shaped as discrete subcommands
instead of a REPL). The spawned PID is recorded in `driver.state.json`
next to the driver, so `quit` only ever kills the instance the driver
itself started — never an instance you (or the user) launched by hand.

### Commands

| command | what it does |
|---|---|
| `launch [exe-path]` | spawn the app detached with CDP enabled, wait for port 9333, record PID. Defaults to `src-tauri/target/debug/BaiyuAISpace2.exe` |
| `screenshot <file>` | `Page.captureScreenshot` → PNG saved to `<file>` |
| `eval "<js>"` | `Runtime.evaluate` in the page, prints the value |
| `nav <route>` | sets `location.hash` directly — e.g. `nav mcp`, `nav local-deploy`, `nav knowledge-base`, `nav history`, `nav settings`, `nav ""` for chat |
| `click "<text>"` | finds a `button` / nav-item / link / `[role=button]` whose text includes `<text>`, dispatches a real `click` event on it |
| `quit` | kills the PID `launch` recorded, clears state |

Override the CDP port with `BAIYU_CDP_PORT` if 9333 is taken.

## Run (human path)

```bash
pnpm tauri dev    # rebuilds + opens a real window, ~25s first run. Ctrl-C to stop.
```

Verified this opens a window titled "BaiyuAISpace" — but it gives you no
programmatic handle on the UI, and it also spawns several `node`
helper processes (vite dev server) alongside the app process. Useless
for an agent; use the driver instead.

## Test

No automated test suite exists in this repo (no `test` script in
`package.json`, no `#[test]` modules in `src-tauri/src`). `pnpm build`
(runs `vue-tsc --noEmit` then `vite build`) and `cargo check` /
`cargo build` in `src-tauri/` are the closest thing to a correctness gate.

## Gotchas

- **Two instances can't share the WebView2 profile.** Launching a second
  `BaiyuAISpace2.exe` while one is already running (debug or release,
  doesn't matter) fails with `failed to create webview: WebView2 error:
  WindowsError(Error { code: HRESULT(0x8007139F), ... })` — and the
  broken instance's process still shows up in `Get-Process` with no
  webview ever rendering. Close every running `BaiyuAISpace2.exe`
  before `launch`. Before closing anything, check whether it's actually
  yours:
  ```powershell
  Get-CimInstance Win32_Process -Filter "Name='BaiyuAISpace2.exe'" | Select ProcessId,CreationDate,CommandLine
  ```
  A release-build path or a creation time that predates your own
  `launch` call likely means the user opened it themselves — ask before
  closing that one; only `driver.mjs quit` (or killing a PID you just
  saw `launch` print) is unambiguously safe to do without asking.
- **`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` only takes effect on a clean
  launch**, not on an already-running instance — confirmed working (`curl
  http://127.0.0.1:9333/json/version` returns real CDP info) once nothing
  else is holding the profile.
- **Not every view has an `<h1>`.** `SettingsView`/`MCPView`/
  `LocalDeployView` use `<h1 class="page-title">`, but `KnowledgeBaseView`
  titles its header with a different element — `eval "document.
  querySelector('h1')?.textContent"` prints `undefined` there even though
  the page rendered correctly. Take a screenshot to confirm navigation,
  don't rely on one selector.
- **Routing is hash-based** (`createWebHashHistory`), so the CDP page URL
  is always `http://tauri.localhost/#/<route>` — `nav` setting
  `location.hash` directly is more reliable than hunting for a sidebar
  link, especially for things not in the nav menu.
- **The app's log file and its app-data folder use different names.**
  Logs land in `%APPDATA%\BaiyuAISpace2\logs\app_<date>.log`, but the
  SQLite db and WebView2 profile live under `%APPDATA%\com.baiyu.aispace\`
  (from `identifier: "com.baiyu.aispace"` in `tauri.conf.json`). Don't go
  looking for the db next to the logs.

## Troubleshooting

- **`failed to create webview: WebView2 error: ... HRESULT(0x8007139F)`**
  in `%APPDATA%\BaiyuAISpace2\logs\app_<date>.log`: another instance has
  the WebView2 profile locked. Close all `BaiyuAISpace2.exe` and retry.
- **`driver.mjs launch` exits with "App did not open the CDP port in
  time"**: same root cause — check `Get-Process -Name BaiyuAISpace2` for
  a stale instance first.
- **`driver.mjs screenshot`/`eval`/`click`/`nav` fail with `No page target
  found`**: the app isn't running with the debug port open. Run `launch`
  first, or check `curl http://127.0.0.1:9333/json`.
- **`cargo build` panics with `The 'frontendDist' configuration is set to
  "../dist" but this path doesn't exist`**: run `pnpm build` first.
