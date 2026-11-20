# pi-star port status

**English** · [简体中文](status.zh-CN.md)

This table lists the current Rust implementation level by pi-web API group. `✅` means the main path
works and is covered by tests, `◐` means the API/file layer works but runtime boundaries remain, and
`○` means not implemented. The task list and acceptance criteria live in
`openspec/changes/port-pi-web-to-rust-backend/`.

The UI has been synced from pi-web `v0.13.0` to `v0.15.0` (except side chats `/side`; `SubagentViewer`
and "Ask in new chat" are kept). The item-by-item comparison, ported scope, and deferred work are in
[`upstream-pi-web.md`](upstream-pi-web.md); local divergences for the next re-sync are in
[`../ui/UPSTREAM.md`](../ui/UPSTREAM.md).

| Group | Status | Notes |
| --- | --- | --- |
| Session scan / incremental index | ✅ | File fingerprint cache + parallel parsing + persisted `pi-star-session-index.json`; ~20ms on a hit |
| Session read / tree / context paging | ✅ | Active branch, tail/before, summary list, projectRoot/branch |
| Session writes | ✅ | rename, delete, delete-many, fork, stats, search, export |
| Session auto-title | ✅ | Generates a title with the current/default model; falls back to local truncation when no model is available |
| Sidebar UI state | ✅ | Locking, atomic writes, revision, backup on corruption |
| Files | ✅ | list/read/download/meta/upload/upload-check/allow-link/watch, DOCX preview |
| @ file index | ✅ | git ls-files with ignore-rule fallback, fuzzy matching |
| Git / worktree | ✅ | status, diff, list/add/remove |
| Live transport | ✅ | one `/api/stream` WebSocket per browser profile: channel subscriptions, cursor resume, backpressure, whole-hub buffer budget, SharedWorker across tabs |
| Agent main path | ✅ | Backend chosen per session and persisted as the `pi-star:agent-backend` custom entry: default `rpc` (`pi --mode rpc`, standard PI), optional `quick` (in-process `AgentHarness`/QuickPI, pi v3 storage, tools sandboxed through AllowedRoots, including MCP and subagents); `/api/agent/backends` and the Agent dropdown in the composer share the same state, and switching is disabled while running. QuickPI renders the system prompt by pi's sections (tools/rules/docs/context/skills/cwd), reads the tool loadout from settings `defaultTools` and `pi-star:tool-selection`, and `set_tools` persists the selection as well |
| PiRpcBackend | ✅ | prompt/steer/follow-up/abort/queue, model/thinking, compaction/retry, stats, fork/clone, session rekey, single-writer routing, subagent session locks, extension UI, event stream; `pi-star-agent-extension` fills in get/set tools, tree, branch, reload, and basic subagent tools; real-pi + fake-model end-to-end tests cover tools, abort, extension UI, fork/rekey. As the default `rpc` backend it only owns sessions that selected it |
| Idle reclamation / leases | ✅ | `PI_WEB_IDLE_TIMEOUT_MS`, background reclaim every 30s; lease renewal |
| Subagents | ✅ | Built-in `general-purpose`/`explore`/`plan`, global/workspace/project file scopes, full pi-web profile fields (display_name, load_skills/extensions, codemode, mcp, max_turns, prompt_mode, color, isolation, persist_session, extension tool selection, etc.), unknown frontmatter preserved, builtin switches and `agents/settings.json`, child session/waiting/concurrency/status display |
| Extension UI | ✅ | Pending requests, replay ids, response/input, blocking timeout; never triggered without a Node extension process |
| Transcript notices | ✅ | cache miss / cache warm / summary cost / thinking dropped; project settings override global |
| Providers | ✅ | OpenAI-compatible / Anthropic Messages; model cache, provider usage |
| OAuth / device-code | ✅ | Anthropic authorization code + PKCE, GitHub Copilot device-code; all other built-in providers sign in through the bundled pi/pi-ai OAuth flow (Node bridge); API key save/delete/sign-out |
| Models and models.json | ✅ | Catalog, default/enabled, CRUD, discover/test, image-size limit validation, models.dev autocomplete, models-store remote refresh |
| MCP | ✅ | File read/write, status/masking, Test, OAuth, mcp-auth.json; agent turns automatically connect stdio/HTTP servers and register `mcp__*` tools |
| Skills | ✅ | Aligned with pi's resource loader: settings `skills` paths, package-internal `skills/`, `~/.pi/agent/skills`, `~/.agents/skills`, project `.pi/skills`, and ancestor `.agents/skills`; returns `sourceInfo`/diagnostics/`projectResourcesLoaded`; list/content/toggle, skills.sh search, install/update, lock file, enabled skills injected into the system prompt |
| Plugins | ✅ | Matches pi package-manager paths (`~/.pi/agent/npm|git`, project `.pi/npm|git`); discovers package-internal extensions/skills/prompts/themes, `pi` manifest, package filters/disable, standalone extensions, counts/totals/diagnostics, local/Git/npm install and update, single and batch enable/disable, update checks |
| Project trust | ✅ | Inheritance, decisions, project MCP manifest |
| Static serving / SPA | ✅ | Cache headers, SPA fallback, API JSON 404 |
| Auth / security | ✅ | Host/Origin, body limit, cookie/Basic Auth, exponential backoff on failures |
| Debug / web-auth | ✅ | Read-only snapshots; sockets, channels by kind, ring bytes, gaps and drops, addresses and browser tabs |
| app-update | ✅ | Configurable release source (`PI_STAR_UPDATE_URL`), 12h cache, supports `PI_STAR_SKIP_VERSION_CHECK=1` |
| Embedded frontend assets | ✅ | Release embeds `ui/dist` with `rust-embed`; debug reads from disk; `--ui-dir` overrides; health response exposes `assets` |
| Desktop runtime contract | ✅ | stdout `assets`/`listening`/`bind-failed` events, stderr logs, bind exit code 2, `--exit-on-stdin-eof` |
| Tauri desktop shell | ✅ | Single instance, locate/launch/health-check, backoff restart, splash/error page, tray, launch at login, close-to-tray, process-tree cleanup |
| Desktop webview | ✅ | Native decorations, stable port, zero IPC in the main window, read-only `__PI_STAR__` marker, PWA/SW skipped in desktop mode; same-origin `window.open` becomes a native window, external links go to the system browser |
| Desktop packaging | ◐ | `package-linux-app/windows/mac` inject `bundle.resources`; installers carry `pi-star`, Node, the pi CLI, and `agent-extension`; Linux deb/AppImage tested, Windows/macOS awaiting on-device acceptance |
| CI / Release | ◐ | Workflows are in place: CI runs Rust/UI/three-platform checks; Release is triggered by `v*` and aggregates artifacts; has not run on GitHub yet |
| Windows bash | ◐ | Windows installers bundle MinGit; probe order: bundled Git/`PI_STAR_BASH` → derived from git.exe → standard install dirs → `where bash.exe` (excluding WSL); `--noprofile --norc -c` + `CREATE_NO_WINDOW`; awaiting Windows on-device acceptance |
| Tests | ✅ | All Rust workspace unit/integration tests (including per-session backend switching, CLI events/stdin/port) + 1705 client unit tests + browser e2e (including PTY terminal and right dock) + `app/` unit tests |

## Runtime boundaries

These are not API gaps but current capability boundaries of the Rust agent runtime:

- **Node/TypeScript extension execution**: extension packages can be installed and managed, and
  `extension_ui_request` transport, responses, and timeouts are implemented; but the Rust agent does
  not execute pi's TypeScript extensions, and the host for extension-registered tools, commands,
  widgets, and extension UI is still missing.
- **Provider OAuth runtime**: the Anthropic and GitHub Copilot flows are implemented natively in
  Rust; other providers reuse the OAuth implementation of the bundled pi's
  `@earendil-works/pi-ai` (Node bridge). Without the bundled Node/pi runtime, Anthropic/Copilot OAuth
  and all API-key providers still work.
- **Session export**: prefers the installed `pi --export` to produce the same HTML as pi; falls back
  to a simplified Rust-generated HTML when `pi` is unavailable.

## Desktop form

`app/` is a separate Tauri 2 workspace: it launches and supervises the bundled `pi-star` and defaults
to the fixed port `127.0.0.1:30151`. The fixed port keeps the webview origin stable (localStorage,
cookies, cache); on a port conflict it fails with a message instead of silently switching ports.

- The main window uses native decorations and has no Tauri IPC permissions; only the built-in splash
  can call `desktop_action`.
- The release `pi-star` embeds the UI, so installers do not need `ui/dist`; debug still reads from
  disk.
- Desktop preferences: `~/.local/share/net.quadhub.pi-star/desktop.toml`; logs: `pi-star.log`,
  `desktop.log`.
- Frameless title bars/`windowControl`, native notifications, auto-update, and signing/notarization
  remain future work.

## Ports

pi-star listens on `127.0.0.1:30151` by default (Vite dev server: 30152), which does not conflict
with pi-web's 30141. The desktop shell also uses 30151 by default; override it with `port` in
`desktop.toml`.
