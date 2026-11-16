# Development and build

**English** · [简体中文](development.zh-CN.md)

This document is for contributors who run, modify, test, and package pi-star from source. To install
the desktop app, go back to the [English README](../README.md) or the
[简体中文 README](../README.zh-CN.md).

## Repository layout

```text
ui/                     pi-star UI (Vite port of pi-web, React 19 + Tailwind 4)
  src/app|components|hooks|lib
  src/shims/            local stand-ins for next/navigation, next/image, parseStreamingJson
crates/pi-star-ai/      models, providers, streaming protocols, usage (merged from rpi)
crates/pi-star-agent/   agent loop, events, queues, tool calls (merged from rpi)
crates/pi-star-tools/   built-in tools and ExecutionEnv (merged from rpi)
crates/pi-star-harness/ session state machine, compaction, system prompt, skills, harness loop
crates/pi-star-core/    sessions (v3), config, models, files/Git, terminal, UiState, agent adapters
crates/pi-star/         Axum routes, SSE, static/embedded assets, auth and security middleware, CLI
app/                    Tauri 2 desktop shell (separate Cargo workspace): launches and supervises pi-star
agent-extension/        pi extension: fills in tools, branches, and subagents for the RPC backend
openspec/               proposals/specs/designs/tasks for the port and desktop work
scripts/                UI sync, desktop packaging, version checks, and runtime preparation
```

Release builds embed `ui/dist` into the `pi-star` binary with `rust-embed`; debug builds keep
reading it from disk so the frontend can be iterated quickly. The desktop shell contains no business
logic: it only starts, health-checks, supervises, and hosts the tray and windows.

## Implementation overview

The main paths that already work:

- 232 client modules copied from pi-web, built with Vite and served same-origin by Axum; pages,
  session sidebar, file tree, chat, model picker, settings panels, and the terminal UI all load.
- Scanning and reading pi JSONL sessions, with session lists, active-branch context, upward paging,
  tree/branches, rename, delete, fork, search, stats, export, and sidebar UI state
  (pins/archive/project order/project names).
- File list/read/download/metadata/upload/upload-check/directory-link confirmation, the `@` file
  index, and file-watch SSE.
- Git status/diff, worktree list/create/remove, and a PTY terminal (input/resize/SSE output).
- Agent runtime: new session, prompt, SSE events, abort/queue, set_model, set_thinking_level,
  set_tools, compact, navigate_tree, fork, stats, last reply, idle reclamation; OpenAI-compatible
  and Anthropic Messages streaming providers; built-in read/write/edit/bash/find/grep/ls tools.
- Subagents: child-session creation, parent waiting, concurrency caps, profile scan/save/toggle,
  and status display.
- Model catalog, models.json, default/enabled models, model cache, provider usage, API key
  save/delete, Anthropic OAuth, GitHub Copilot device-code, OAuth for other providers through the
  bundled pi/pi-ai flow, tools/cache-miss settings, project trust, MCP file read/write/test/OAuth,
  skills list/content/toggle/search/install/update, plugin install/update/enable/disable, DOCX
  preview, transcript notices, debug snapshots, password login, and the app-update endpoint.

All 61 OpenSpec tasks are complete. The per-pi-web-API implementation table and runtime boundaries
(Node extension execution, MCP tool registration during agent turns, the full skill resource loader)
are in [`status.md`](status.md).

## Agent backends

The backend is chosen per session. The default is standard PI (Node.js), executed as a
`pi --mode rpc` subprocess:

| id | Name | Implementation |
| --- | --- | --- |
| `rpc` | Standard PI (Node.js) | `PiRpcBackend`, `pi --mode rpc`, compatible with TypeScript extensions |
| `quick` | QuickPI | In-process `AgentHarness` (`crates/pi-star-agent/tools/harness/ai`) |

The choice is stored in the session file as the `pi-star:agent-backend` custom entry, travels with
the session file, and is ignored by pi. Without that entry the session is treated as standard PI.
Both backends share the same session files and configuration, but a session can only be written by
one backend at a time.

- `GET /api/agent/backends` returns the available options.
- `POST /api/agent/new` accepts `backend: "rpc" | "quick"` for the new session.
- The session command `{"type":"set_backend","backend":"rpc|quick"}` switches while idle and returns
  409 while running.
- The Agent dropdown in the composer and the `backend` field of `/api/agent/:id` use the same state.

`PI_STAR_PI_CLI` selects the pi executable, `PI_STAR_AGENT_EXTENSION` selects
`agent-extension/index.js`, and `PI_STAR_PI_RPC_MAX_SESSIONS` caps concurrent pi RPC sessions
(default 8; idle sessions are reclaimed first). PiRpcBackend covers prompt, steer, follow-up, abort,
queues, model/thinking, compaction, retry, session stats, fork/clone, commands, extension UI, and the
event stream; `get_tools`, `set_tools`, `navigate_tree`, `fork_branch`, and `reload` are filled in by
`agent-extension/` through the pi extension API. That extension also registers basic `Agent`,
`get_subagent_result`, and `steer_subagent` tools; subagents are separate pi RPC sessions.

QuickPI renders the system prompt by pi's sections
(preamble/tools/rules/docs/addendum/project_context/skills/cwd) and reads `SYSTEM.md`,
`APPEND_SYSTEM.md`, `AGENTS.md`/`CLAUDE.md`, and `defaultTools` from `settings.json`; the
`pi-web:tool-selection` custom entry restores the tool selection when a session is reopened.
Settings › Skills, Sub-agents, and Plugins use the same resource discovery rules as pi: settings
paths, package resources, `~/.agents`, project `.pi`, and `npm`/`git` install directories.

### Data files

Files remain the single source of truth:

- Sessions: `~/.pi/agent/sessions/<encoded-cwd>/*.jsonl`
- Settings: `~/.pi/agent/settings.json`, `models.json`, `auth.json`
- Sidebar state: `~/.pi/agent/pi-web-session-state.json`
- Project resources: `<cwd>/.pi/mcp.json`, `.pi/skills`, and `~/.pi/agent/skills`

pi-star, the pi CLI, and pi-web read the same sessions; after stopping pi-star you can continue with
any of them.

## Build and run

Requirements: Rust stable (1.88+) and Node.js 22+ (needed to build the UI, package the desktop app,
and prepare the bundled pi runtime).

```bash
# 1. Build the UI
cd ui
npm install
npm run build
cd ..

# 2. Build and run the server (release embeds the ui/dist from step 1)
cargo build --release
./target/release/pi-star --port 30151
```

Open <http://127.0.0.1:30151>. The release binary can be copied to a directory without `ui/dist`;
debug builds read `ui/dist` from the repository by default, so after frontend changes you only need
`npm run build` instead of rebuilding Rust.

The browser is not opened automatically; pass `--open` if you want that. Standard output only
contains lifecycle events prefixed with `pi-star: ` (`assets`, `listening`, `bind-failed`); logs go
to standard error; when the port is taken the process exits with code 2.

Common flags:

```bash
pi-star --port 8080 --hostname 0.0.0.0
pi-star --open
pi-star --ui-dir ui/dist --agent-dir ~/.pi/agent
pi-star --port 0 --exit-on-stdin-eof   # ephemeral port + exit when the parent disappears
```

`--ui-dir` (environment variable `PI_STAR_UI_DIR`) is an explicit override: when set, the UI is
always read from disk and `index.html` is validated at startup; when unset, release builds use the
embedded assets and debug builds use `ui/dist`.

Related environment variables: `PORT`, `PI_WEB_HOSTNAME`, `PI_STAR_OPEN_BROWSER`, `PI_STAR_UI_DIR`,
`PI_WEB_PASSWORD`, `PI_WEB_ALLOWED_HOSTS`, `PI_CODING_AGENT_DIR`. Desktop installers additionally set
`PI_STAR_PI_RUNTIME` (bundled Node + pi), `PI_STAR_AGENT_EXTENSION`, and `PI_PACKAGE_DIR`;
`PI_STAR_PI_CLI` switches to the user's own pi, and `PI_STAR_BASH` overrides the Windows bash path.

### Development

```bash
# Terminal 1: Rust server (30151)
cargo run -p pi-star

# Terminal 2: Vite (30152, /api proxied to 30151)
cd ui && npm run dev
```

### Verification

```bash
cargo test --offline -p pi-star-core
cargo test --offline -p pi-star
cd ui && npm run typecheck && npm run build && npm test
# requires a running pi-star (default http://127.0.0.1:30151)
npm run smoke
```

`npm test` runs the client unit tests migrated from pi-web (Node test runner + jiti). Browser
regression tests start `target/{debug,release}/pi-star` themselves; build the UI and the binary
first:

```bash
cd ui && npm run build
cd .. && cargo build
cd ui && npm run test:e2e        # sessions, paging, sidebar, settings, models, files, themes, ...
npm run test:terminal            # real PTY terminal, reconnect, restart, workspace isolation
```

Playwright needs Chromium. If the browser is not in the default location, set `PI_STAR_CHROMIUM`,
for example `PI_STAR_CHROMIUM=/usr/bin/chromium-browser`; `PI_STAR_BIN` selects the binary.
pi-web's extension-dialog e2e needs Node/TypeScript extension execution and is skipped by default;
set `PI_STAR_EXTENSIONS=1` to re-enable it once a Rust extension host exists. `PI_STAR_URL`
overrides the already-running server URL for `npm run smoke`.

Tests migrated from pi-web live in `ui/src/**/*.test.mjs` and `ui/e2e/`. Tests that only existed for
pi-web's server (Next API routes, Agent RPC, the MCP host, etc.) are covered by Rust unit/integration
tests: `cargo test -p pi-star-core` and `cargo test -p pi-star`.

### Re-syncing the UI

```bash
PI_WEB_DIR=/path/to/pi-web node scripts/sync-ui-from-pi-web.mjs
```

The script only copies client modules reachable from `app/page.tsx`, `app/login/page.tsx`,
`AppShell`, and `PwaRegistration`; it never brings over pi-web's API routes or Node-only modules.
Intentional local divergences and the edits to re-apply after a re-sync are listed in
[`../ui/UPSTREAM.md`](../ui/UPSTREAM.md).

## Desktop app

`app/` is a separate Tauri 2 workspace (the root workspace has `exclude = ["app"]`) and is
responsible for:

- single-instance startup, locating and launching the bundled `pi-star`;
- confirming process ownership through stdout events + `/api/health`, supervising crashes, and
  restarting with 1/2/4/8/16 second backoff;
- the splash/error page, the natively decorated main window, the tray, launch at login, and
  close-to-tray;
- cleaning up pi-star and its Node/PTY child process tree on exit.

Installers bundle Node, the pi CLI (`@earendil-works/pi-coding-agent`), and `agent-extension/`;
Windows also bundles MinGit, so users do not need Node, pi, or Git for Windows. Versions are pinned
in `packaging/pi-runtime.json`.

The main window loads `http://127.0.0.1:<port>/`; the port defaults to `30151` and is persisted in
the desktop preferences. Keeping the port stable matters because webview localStorage, cookies, and
caches are keyed by origin; on conflict the shell shows an error instead of silently switching ports.
The main window uses native decorations and has no Tauri IPC permissions; only the built-in splash
can retry, quit, or open logs.

Development:

```bash
# Terminal 1: build debug pi-star once
cargo build -p pi-star
# Terminal 2: build the UI (the debug shell reads it from disk)
cd ui && npm run build && cd ..
# Terminal 3: run the desktop shell (locates the repo's target/debug/pi-star)
cd app && cargo tauri dev
```

UI hot reload: first run `cd ui && npm run dev` (30152), then start the debug shell with
`PI_STAR_DESKTOP_URL=http://127.0.0.1:30152/`; that mode skips spawn and supervision.

Packaging (current platform):

```bash
./scripts/check-desktop-version.sh
./scripts/package-desktop.sh          # current platform
./scripts/package-linux-app.sh        # deb + AppImage
./scripts/package-windows.sh          # NSIS + MSI + portable zip (Git Bash)
./scripts/package-mac.sh              # .app + dmg
```

The script order is: prepare `pi-runtime` (Node + pi + agent-extension + Windows MinGit) → build the
UI → build the release `pi-star` with embedded UI → Tauri installers. Before releasing, add the
changes under `## [<version>]` in `CHANGELOG.md`; the release body is generated from that section.
`pi-star` and `pi-runtime/` are injected through a temporary `bundle.resources`: on Linux they land
in `/usr/lib/PiStar/`, on macOS in `Contents/Resources/`, and on Windows next to `PiStarApp.exe`.
Artifacts go to `dist/<os>/`. Linux **only ships the desktop app**; there is no separate server
package anymore.

On Windows the `bash` tool prefers the bundled MinGit (no fallback to `cmd.exe` and no WSL
`System32\bash.exe`); system Git for Windows and the `PI_STAR_BASH` override are also supported, see
[`../packaging/README.md`](../packaging/README.md). Official macOS releases are signed and notarized;
Windows installers are not code-signed. Signing secrets and CI/release configuration live in
`.github/workflows/`.

Desktop shell data directory (Linux `~/.local/share/net.quadhub.pi-star/`):

- `desktop.toml`: `autostart`, `close_to_tray`, `port`;
- `pi-star.log`: child stderr and event logs (rotated at startup when larger than 5 MB);
- `desktop.log`: shell preference/autostart errors.

pi-star sessions and settings still live in `~/.pi/agent`, shared with the browser form and the pi
CLI.

## Security model

- Listens on `127.0.0.1` only by default. Set a long random `PI_WEB_PASSWORD` when binding to a LAN.
- File/Git/terminal APIs only allow the session cwd, projectRoot, roots explicitly allowed by this
  process, and user-confirmed directory-link targets; paths are checked again after resolving
  symlinks.
- Requests with an `Origin` header are checked for same-origin; `Host` only accepts loopback,
  private ranges, or `PI_WEB_ALLOWED_HOSTS`.
- Passwords are stored in an HttpOnly, SameSite=Strict cookie; API clients can use Basic Auth
  (username `pi`).
- Plain HTTP provides no encryption; use an HTTPS reverse proxy or a trusted VPN for remote access.
- MCP/stdio, the terminal, and tool execution all start child processes; only use this with trusted
  projects.

## License

pi-star is released under the **Apache License 2.0** ([`../LICENSE`](../LICENSE)). Portions copied
from pi-web and pi-rust remain under MIT; attributions, the full MIT text, and font licenses are in
[`../NOTICE`](../NOTICE). Sources, mappings, and local changes are documented in
[`upstream-pi-web.md`](upstream-pi-web.md), [`upstream-rpi.md`](upstream-rpi.md), and
[`../ui/UPSTREAM.md`](../ui/UPSTREAM.md).
