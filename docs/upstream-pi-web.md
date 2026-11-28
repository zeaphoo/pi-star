# pi-web 0.13 → 0.15: portability comparison

**English** · [简体中文](upstream-pi-web.zh-CN.md)

This document compares pi-web `v0.13.0` (pi-star's UI baseline) with `v0.15.0` and marks which changes
can be ported directly to pi-star, which need matching Rust backend work, and which do not apply.

## Baseline confirmation

- pi-star's `ui/src` is byte-for-byte identical to the output of syncing pi-web `v0.13.0` with
  `scripts/sync-ui-from-pi-web.mjs`, plus 44 local divergences: 37 modified files (branding/version/
  i18n, backend selection, desktop and test adaptations, etc.) and 7 local additions (`main.tsx`,
  `shims/`, `types/`, `vite-env.d.ts`, `components/AgentBackendSelector.tsx`, `lib/desktop.ts` and
  its tests).
- 0.13 has 231 client-reachable modules; 0.15 has 235: adds `SessionChatPanel.tsx`,
  `side-tab-state.ts`, `side-chat-client.ts`, `favicon-badge.ts`, `html-sandbox-polyfill.ts`,
  `panel-tab-order.ts`, and removes `SubagentViewer.tsx`, `subagent-viewer-state.ts`.
- 0.15 also bumps the dependency from pi 1.4.0 to **pi 1.5.0** (Next 16.3.8 → 16.4.4, irrelevant to
  pi-star). pi-star now packages **pi 1.7.0** (`packaging/pi-runtime.json`); see the
  "pi 1.5.0 → 1.6.0" section at the end.
- How this was verified: copy the sync script to a temporary directory, point `PI_WEB_DIR` at
  worktrees of `v0.13.0` / `v0.15.0`, run it against each, then `diff -rq`. The conclusion does not
  depend on upstream git history.

## Summary

| Feature | Upstream | Type | Recommendation |
| --- | --- | --- | --- |
| Right-panel tab drag reordering | `5e0e47f` #1511 | Frontend only | Port directly |
| Tab favicon unread badge | `0b1eeff` #1484 | Frontend only | Port directly |
| Quote inserts only the selected text (blockquote) | `6d0946b` #1502 | Frontend only | Port directly ("ask in side chat" goes with side chats) |
| Git changes list/tree view + memory | `2251c3f` #1490 | Frontend only | Port directly |
| File preview: prose wraps by default | `f07f368` #1471 | Frontend only | Port directly |
| Storage shim for HTML sandbox preview | `5269cb0` #1494 | Frontend only | Port directly |
| Mobile touch sizes / Agents button | `57078bb` #1506 | Frontend only | Port directly |
| Mobile Settings level navigation + system Back | `9afc508` #1510 · `1420b81` · `1615347` | Frontend only | Port directly |
| Model panel drag / Alt+arrows / A–Z sort | `6386fa4` | Frontend only | Port directly |
| Custom messages keep the TUI text layout | `b25fa0a` | Frontend only | Port directly |
| Extension UI ANSI palette | `da6d538` | Half frontend | Port the client half |
| Stable streaming token / t/s indicator | `79425f8` #1520 | Frontend only | Port directly |
| Narrow-screen message actions / composer icons | `cb3cebe` #1487 · `3bad742` | Frontend only | Port directly |
| Paste-loss fix (Chrome) | `8d2fb36` #1517 | Frontend only | Port directly |
| Notification dedupe (page + push ring once) | `5f31fb6` #1518 | Frontend + Rust push | Needs matching sw/Rust changes |
| PWA manifest with credentials | `b54a63d` #1477 | Frontend + Rust route | Port directly (different approach for Vite) |
| Model picker scoped/all switch | `14c0c94` #1503 | Frontend + `/api/models` | Needs backend fields |
| Provider display name instead of id | `f6c8699` #1514 | Frontend + `/api/models`, `/api/auth/providers` | Needs backend fields |
| Signed-out providers' enabledModels count as unmatched | `7977daf` #1529 | Frontend lib + enabled-models route | Needs backend work |
| Switch models while running | `7a815d1` #1527 | Frontend + agent routes | Small change, needs RPC verification |
| Windows terminal prefers PowerShell | `fff941a` #1508 | Rust terminal | Small change |
| List recent subagents when the id is not found | `6fbb36b` #1491 | agent-extension (JS) | Port directly |
| Reconcile run state after a subagent restart cuts it off | `98ad390` #1473 | Rust runtime | Medium |
| Keep tool order to hit the prompt cache | `c319c85` #1481 | Rust tool selection | Medium, low priority |
| Empty project group Pin / archive empty labels | `2b7c403` · `b12b908` | Frontend only | Port directly |
| Side chats (`/side`) | `3bad742` #1469 + follow-ups | Large feature | Separate project |
| Docker / Next 16 / shared model runtime | `e97d00b` · `ad1b381` · `15c52bb` | Not applicable | Skip |
| Remove export HTML patch | `1fbf6c6` | Not applicable | pi-star has its own export, no patch |
| Extension UI theme (server half) | `da6d538` | Not applicable | pi-star does not execute TS extensions |

## A. Frontend only, direct migration

These only touch `ui/src`, not Rust. You can take them file by file from 0.15, remembering to
re-apply local changes.

1. **Right-panel tab drag reordering**: `lib/panel-tab-order.ts` (new), `components/TabBar.tsx`
   (HTML5 drag + drop indicator), and `panelTabOrder` state in `AppShell.tsx`. The order lives only
   in memory; closed tabs are removed from it and reopened tabs are appended.
2. **favicon unread badge**: `lib/favicon-badge.ts` (new; draws the badge on canvas with the same
   color as the sidebar status dot), `SessionSidebar.tsx` (feeds it the awaiting/unread counts), and
   a new `hiddenFinishedSessionIds` in `AppShell.tsx` (sessions that finished in a background tab and
   should not count as unread in the sidebar).
3. **Quote inserts only the selected text**: `buildQuotedSelection(text)` in
   `lib/quoted-selection.ts` now emits only `> ...` plus a blank line, with a new
   `quoteSeparator()`; `ChatInput` gains `insertQuote`; `ChatWindow` drops `sourceEntryId` and
   "Ask in new chat". Note that "Ask in side chat" depends on side chats; if those are deferred,
   port only the quote part.
4. **Git changes list/tree**: `ChangesTreeRows`/`ChangesFolderRow` and the view toggle in
   `FileExplorer.tsx`; `pi-star:sidebar-changes-view-mode` (localStorage) in
   `lib/sidebar-prefs.ts`. The tree build reuses `lib/search-tree.ts`; row height moves to
   `--file-row-height`.
5. **Prose wraps by default**: `wrapsByDefault()` / `resolveInitialWrapLines()` in
   `lib/file-viewer-state.ts`; `wrapLines` becomes optional (`undefined` means the user never chose);
   the diff view in `FileViewer` also supports wrapping.
6. **HTML preview storage shim**: `lib/html-sandbox-polyfill.ts` (new) injects in-memory
   `localStorage`/`sessionStorage` into `srcDoc`, fixing exceptions when the preview reads/writes
   storage under an opaque origin.
7. **Mobile**: `app/globals.css` adds `--top-bar-height` (36 / 44px), `--file-row-height`
   (24 / 36px), and hides `.message-action-label` on small screens; `AppShell` moves the Agents
   button out of the More menu next to the others and unifies icon sizes; `ChatInput` gets
   `controlSize`; `MessageView` wraps action labels in `<span className="message-action-label">`.
   The `.chat-composer-row` container query (Steer / Follow-up become icons when the panel is narrower
   than 480px) arrives with side chats, but it is CSS-only and can land earlier.
8. **Mobile Settings**: `usePhoneSettingsLevel` in `SettingsPanel.tsx` (home/section/detail levels,
   `history.pushState` so system Back goes up one level, `history.go(-pushed)` on close),
   `animatePhoneLevelChange` (forward/back animation respecting `prefers-reduced-motion`), and
   `ConfigDetailNavigationContext` in `components/SettingsUi.tsx`; plus `app/settings.css` and
   `openAtHome` in `AppShell`. Keep the current section when the desktop window narrows.
9. **Model panel sorting**: `moveModelEntry` / `modelDropIndex` / `sortModelEntries` (natural,
   stable) in `components/models-config-helpers.ts`, and row dragging, Alt+↑/↓, and the A–Z button in
   `ModelsConfig.tsx`. models.json is saved whole through the existing write API; no Rust change is
   needed.
10. **Custom message text layout**: `remarkTextLayout` in `lib/markdown.ts` restores paragraph
    indentation from the source text, `MarkdownBody` gains `keepTextLayout`, and `MessageView` uses it
    for custom messages; `.markdown-custom-message` in `globals.css` switches to monospace +
    `pre-wrap` + paragraph spacing, with `@font-face` `unicode-range` covering arrows/tabs
    (`Terminal Drawing`).
11. **Extension UI ANSI palette (client half)**: `components/AnsiText.tsx` enables
    `ansiUp.use_classes = true`, and `globals.css` adds the 16-color `--ansi-*` and
    `.ansi-*-fg/-bg` (light and dark sets, taken from `lib/terminal-theme.ts`). The server half
    (`lib/extension-ui-theme.ts`, giving extension components a pi `Theme`) only matters once pi-star
    actually runs TS extensions in-process; see the runtime boundaries in `docs/status.md`.
12. **Stable streaming metrics**: `MessageView.tsx` extracts `AssistantMessageHeader`; the model name
    is always shown, token counts and t/s only appear while streaming, and widths are reserved with
    `minWidth: calc(6ch + 1px)` / `9ch + 1px` so growing numbers do not jitter the row.
13. **Misc fixes**: Skills search no longer regroups by dormancy (`ChatInput.buildSlashCommandLayout`
    takes an extra `query`); the directory picker no longer closes when dragging outside the
    component and disables selection while navigating; the model provider picker survives IME
    Escape; provider sign-in feedback is localized (i18n keys below); empty sidebar project groups
    show "Pin to keep"; collapsed archive empty labels are hidden.
14. **PWA manifest**: 0.13→0.15 replaces `app/manifest.ts` with `app/manifest.webmanifest/route.ts`
    and adds `crossOrigin="use-credentials"` to `<link rel="manifest">`. pi-star's manifest is a Rust
    route (`routes/misc.rs::manifest`); only add `crossorigin="use-credentials"` to the link in
    `ui/index.html`, and change that route's content type to `application/manifest+json` (it
    currently goes through `no_store(json!())`, which is `application/json`).

New i18n keys (one copy each for en / zh-CN / zh-TW): `sideChat.*`, `chat.commandSide`,
`chat.askInSideChat`, `chat.sideChat*`, `chat.selectionActions`, `chat.modelScopeAll`,
`chat.modelScopeScoped`, `chat.selectModel`, `chat.switchingModel`, `files.viewAsTree`,
`files.viewAsList`, `settings.backTo`, `sidebar.pinToKeep`, `sessionPanel.*`, `models.serverError`,
etc. pi-star's i18n has substantial local changes (branding, desktop copy), so merge by key instead
of replacing whole files.

## B. Needs matching Rust backend work

### 1. Model API: `providerName` / `isScoped` / `?scope=all`

Upstream `f6c8699`, `14c0c94` #1503.

- `modelList[]` in `GET /api/models` gains `providerName` (the display name from the runtime
  provider's `name`; models.json `name` is also supported) and returns `isScoped`.
- Add `?scope=all`: ignore `enabledModels` and return every available model (the cache key must
  distinguish `cwd#all`). The client lazy-loads when "all" is selected or the current model is not in
  the scoped list.
- `GET /api/auth/providers` gains `providerNames` (all runtime providers, including those with no
  chat models).
- `enabled-models`: use the provider display name for `name`; change `stalePatterns` to "matches no
  available model" (`7977daf` removed the `known` concept; signed-out provider entries also count as
  stale, and `prune` removes them).

pi-star today: `ModelListItem` in `crates/pi-star-core/src/models.rs` only has
`id/name/provider(/input)`, ordered by `provider + name` (upstream uses `orderSelectorModels`);
the provider `name` in `routes/settings.rs::enabled_models_view` is just the id, `stalePatterns` is
always `[]`, and there are no pins. Complete the view while doing this item so the client panel works
fully.

### 2. Switch models while running

`7a815d1` #1527. The client only changes `hooks/useAgentSession.ts`: while running
(`agentRunningRef` or `bashRunningRef`), do not call `loadSession()` (it would replace messages from
the stream); instead read the live state from `/api/agent/:id` to sync model/thinking and fall back to
the file after the run ends. pi-star already has `syncLiveModel` and `agentStatusUrl`, so this can be
adopted almost verbatim.

Rust side: `set_model` in `POST /api/agent/:id` is currently passed straight through to RPC/QuickPI
with no visible running-state rejection. Two things need testing: pi's `--mode rpc` accepting
`set_model` during a run, and QuickPI's agent loop reading the current model on each request. If both
pass, it can be enabled without a new endpoint.

### 3. Windows terminal shell

`fff941a` #1508. On Windows, pi-web prefers the PowerShell resolved by pi's `getPowerShellConfig()`
(PowerShell 7 → Windows PowerShell, preserving the Store WindowsApps alias) and falls back to
`ComSpec`/cmd only on failure. pi-star's `crates/pi-star-core/src/terminal.rs` currently does "`SHELL`
env var → `powershell.exe` on Windows → `/bin/sh`": no PowerShell 7 preference, no `where` probe, no
cmd fallback (a `SHELL` pointing at Git Bash is used directly). It can be rewritten to follow the
upstream order without depending on pi's JS API.

### 4. Subagents

- `6fbb36b` #1491: when `get_subagent_result` cannot find the id, list up to 10
  `sessionId: description` entries from the most recent tool result `details` in this session.
  pi-star's `agent-extension/subagent.js` matches the upstream structure and can be ported directly.
- `98ad390` #1473 (**deferred**): upstream reads files when "the child session's own turn ends" and
  writes `completed/aborted/failed` from the last assistant stop reason, fixing runs stuck in
  `running/queued` after a restart cut them off. pi-star's RPC subagents are spawned directly by
  `agent-extension/subagent.js`, which writes neither `pi-star:subagent` metadata nor
  `pi-star:subagent-status` (QuickPI only writes `running` in metadata), and pi-star's subagent panel
  is still the read-only `SubagentViewer`, so users cannot "send a turn inside a child session" — the
  scenario does not exist yet. Revisit once the subagent runtime lifecycle is complete or there is an
  interactive panel.
- `lib/subagent-extension.ts` changed in 0.15 to take `Type` from `typebox` (pi 1.5); that file is
  not in pi-star's client sync scope. The agent-side `agent-extension/` uses plain JSON Schema, so it
  is unaffected by the pi-schema switch.

### 5. Tool order and prompt cache

`c319c85` #1481 (**deferred**). `resolveActiveToolNames()` currently outputs in "`carry` order":
tools recorded in the transcript first (still selected), then newly selected ones, rather than
`selected` first. pi declares tools in this order; changing it rewrites the `<tools>` section and
loses the provider's prompt cache. pi-star's RPC backend passes the client-computed `toolNames`
straight to the extension, so the order never goes through the server; an equivalent fix would have
to happen on the client `tool-presets`/`set_tools` path, and the only benefit is cache hits, hence
deferred.

### 6. Notify only once

`5f31fb6` #1515. The page's own event stream shows a notification, then the Web Push for the same
completion shows another (same tag + `renotify` rings again). Upstream adds `shownAt` to the
notification data; when the sw receives a push it looks for a notification with the same tag and
silently replaces it with `renotify: false` if it is within 10s. pi-star sends push from Rust
(`crates/pi-star-core/src/push.rs`), and `ui/public/sw.js` is synced; both sides must change
together: frontend notification data carries `shownAt`, sw adds `hasRecentNotification()`, and the
Rust sender only needs to keep the tag consistent.

### 7. enabledModels for signed-out providers

`7977daf` #1529, see the `prune/stale` part of B.1. pi-star's simplified view has no `known`
concept; implement "matches no available model = stale" directly without migrating old semantics.

## C. Side chats (`/side`)

Upstream `3bad742` #1469 (ADR `docs/adr/0007-side-sessions.md`), followed by `5644bcf` #1512
(tabs restored with the main session), `502cc16` #1507 (remove from the Agents panel), `48a158b`
(delete confirmation row height), and `6d0946b` (quote popover becomes "ask in side chat"). This is
the only large feature between 0.13 and 0.15 and deserves its own project.

Data model:

- A side chat is a copy of the sidebar Fork: the same `forkSessionBranch` copy logic plus
  `{side:true,name?}` on `POST /api/sessions/[id]/fork`.
- The copied file writes a `pi-web:side` custom entry on line 2
  (`{version:1,parentSessionId,createdAt}`) and chains the original first entry's `parentId` under it;
  it appends a `custom_message` (`customType: pi-web:side-boundary`, `display:false`) as a hidden
  boundary; before the boundary it writes the tool pin again (`pi-web:tool-selection`) so the side
  chat does not inherit an oversized tool set when the pin is lost on another branch of the parent.
- The relation is `{kind:"side",parentSessionId}` and is hidden like subagents: no sidebar row,
  counted in the Agents badge, follows the parent's pin/archive, deleted with the parent, and can be
  deleted on its own.
- Stats only count entries after `createdAt` (`sideOwnEntries()`), otherwise the parent's usage is
  counted twice; search skips side chats; fork/clone refuses side chats (`side` refusal).
- Tools: parent tools ∩ `SIDE_TOOLS` (`read, bash, powershell, grep, find, ls`) as pi's allow-list;
  extension/MCP/Code mode/subagent tools are not registered; chat-only parents keep no tools.
- Client: `/side` is a builtin slash command (usable while streaming), `ChatInput` gains a side-chat
  button, `ChatWindow` renders the boundary message as a separator, the right panel's
  `SessionChatPanel` reuses `ChatWindow` (panel mode, not stealing global shortcuts),
  `side-tab-state.ts` stores side-chat tabs in sessionStorage per main session, `AgentSessionPanel`
  gains "New side chat" and delete, and `SessionChatPanel` replaces `SubagentViewer` (subagents also
  become interactive chat panels).

Key implementation points for pi-star:

1. Add a `Side` variant to `SessionRelation` (`crates/pi-star-core/src/session.rs`) and expose it on
   `SessionInfo`; recognize `pi-web:side` in `info()`.
2. **Watch the summary index**: in summary mode `session_index.rs` calls `read_header_only_info()`,
   reading only the first header line, so it cannot see the side entry on line 2 (the existing
   subagent relation has the same problem and relies on `details_pending` to fill in later). Upstream
   writes the side entry on line 2 and uses a bounded header read; pi-star needs to extend
   `read_header_only_info` to read the first N lines, or let side chats appear as forks in the list
   summary via `parentSession` and correct the relation after details load. The index caches the whole
   `SessionInfo`, so adding fields/enums requires bumping `IndexFile.version` (currently 1);
   otherwise old caches fail to deserialize or return stale relations.
3. Fork route: `routes/sessions.rs` has two paths (active sessions use pi RPC's `fork`, otherwise Rust
   copies the file). Both must support `side:true`: after copying, rewrite the file (line-2 metadata,
   boundary entry, tool pin), and never race a running parent session — upstream copies at the file
   level and never touches a running wrapper.
4. Session read/stats/delete/search: relation in `GET /api/sessions/:id`, own entries in
   `compute_stats`/`total_active_ms`, parent/child folding in `session-delete`, filtering in
   `sessions/search`.
5. Agent open: the RPC backend already supports `--tools <csv>` when spawning `pi --mode rpc`
   (`pi_rpc.rs::open_path` has that branch), which is exactly the allow-list side chats need; compute
   `pin ?? recorded tools` ∩ `SIDE_TOOLS` from the session file and pass it. QuickPI needs to set the
   same tool set when opening the session and refuse `set_tools`/`navigate_tree` adding tools back.
6. Port the client files beyond group A: `session-family.ts`'s `side`/`isHiddenRelation`/
   `sideChatTitle`, `types.ts`, `side-chat-client.ts`, `side-tab-state.ts`, `SessionChatPanel.tsx`,
   `AgentSessionPanel.tsx`, `TabBar.tsx`, `SessionSidebar.tsx`, `ChatWindow.tsx`, `ChatInput.tsx`,
   `useAgentSession.ts`, `session-stats-format.ts`. This spans 16 files and will conflict with local
   changes (i18n, AppShell backend selector); do it on a separate branch.

## D. Not applicable

- **Docker** (`e97d00b`, `aa4be54`) and **Next 16.4.4** (`ad1b381`): pi-star is Vite + Axum + Tauri,
  with no counterpart.
- **Shared model runtime one-hour TTL** (`15c52bb`): a Node in-process model runtime; pi-star's model
  catalog is cached by Rust itself (5s) and refreshed from `models.dev`, a different semantic.
- **Remove export HTML patch** (`1fbf6c6`): pi 1.5.0's template now iterates instead of overflowing
  the stack. pi-star prefers `pi --export` and benefits automatically from the pi upgrade; the
  simplified Rust export has no such patch and needs no action.
- **`lib/extension-ui-theme.ts` server half**: requires executing pi's TS extensions in-process;
  pi-star's `docs/status.md` already lists "Node/TS extension execution" as a runtime boundary. The
  client palette (A.11) can land first.

## pi 1.5.0 → 1.6.0 (done)

pi-web 0.15 depends on pi 1.5.0; pi-star now packages **pi 1.6.0** (`packaging/pi-runtime.json`).
The upgrade changed only the pins file and re-ran `scripts/prepare-pi-runtime.mjs`; the release
workflow caches `pi-runtime` on that file, so the bump invalidates the cache automatically.

What was checked against the staged 1.6.0 runtime:

- `agent-extension/` host commands over `pi --mode rpc`: `get_tools`, `get_system_prompt`,
  `get_context_usage`, `set_tools`, and `reload` (including reconnecting after the reload).
- RPC commands pi-star sends: `get_state`, `get_messages`, `get_entries`, `get_session_stats`,
  `get_last_assistant_text`, and `set_session_name`, with unchanged response shapes.
- Reading an existing session v3 file written by pi-star/QuickPI, including its custom entries.
- Loading the real `~/.pi/agent` config: 59 available models, current model and thinking levels.

Not covered by this upgrade check: an actual streamed model turn, the session export path, and the
Windows/macOS installers; run those before a release.

Version-specific points that touch pi-star:

- Tool argument validation moved to pi-schema in 1.5. `agent-extension/` tools are plain JSON
  Schema, so no TypeBox-only features are lost.
- OpenAI-compatible requests omit `"tools": []` when tools are disabled (1.5.1). pi-star's Rust
  provider code serializes its own requests, so this only affects the bundled CLI path.
- `agent_settled` gained `aborted` (already handled by `pi_rpc.rs`), and RPC
  `new_session`/`switch_session`/`fork` no longer run `session_start` twice but now forward
  extension events such as `entry_appended` (additive).
- 1.6.0 makes extension `render()` return `readonly string[]` and reuse cached containers.
  `agent-extension/` registers no TUI renderers, so it is unaffected.
- pi 1.5+ defaults MCP OAuth to `clientRegistration: "auto"` / CIMD. pi-star's Rust sign-in always
  does dynamic registration and errors when a server advertises no registration endpoint; this gap
  predates the bump and is tracked separately.
- Side chats themselves do **not** require a pi bump: the `getCurrentTools` they use already exists
  in 1.4.0.

## Suggested sync process

1. Do group A (frontend only) first, taking files from `v0.15.0` and re-applying local changes; run
   `npm run typecheck && npm test` after each batch (pi-star has migrated 178 client tests, and group
   A's new tests can be ported directly: `AppShell.favicon-badge.test.mjs`,
   `AppShell.side-tabs.test.mjs`, `ModelSelector.test.mjs`, `ModelsConfig.test.mjs`,
   `panel-tab-order.test.mjs`, `favicon-badge.test.mjs`, `html-sandbox-polyfill.test.mjs`,
   `side-tab-state.test.mjs`, settings-related tests, etc.).
2. Then group B (small backend changes): add the three model API fields first, then running model
   switching and the Windows terminal.
3. Start side chats as a separate project, split into "file layer → read-only display → interactive
   panel" phases, each independently verifiable.
4. Consider adding two capabilities to `scripts/sync-ui-from-pi-web.mjs`: `--report` (list differing
   files against a target version without writing) and recording the upstream tag in
   `ui/UPSTREAM.md` (it currently only records the module count, so the baseline version is not
   visible).

## Appendix: 0.13 → 0.15 commit index

```
2621aa1 fix(directory-picker)      f07f368 feat(file-viewer) prose wrapping
98ad390 fix(subagents) reconcile   c319c85 fix(tools) tool order
0b1eeff feat(tab) favicon badge     cb3cebe fix(chat) narrow actions
b54a63d fix(pwa) manifest creds     e97d00b feat: docker
15c52bb fix(models) runtime TTL     2b7c403 fix(sidebar) empty group pin
6fbb36b fix(subagents) candidates   3bad742 feat(side) side chats
6d0946b feat(chat) quote text only  5269cb0 fix HTML sandbox storage
6265065 / 4bb6d2f models i18n       7b5cd68 fix(chat) skill sorting
2251c3f feat(files) changes tree    c2ef9a5 fix(files) disable select while navigating
c6789c5 fix(models) IME             b25fa0a fix(chat) custom message layout
14c0c94 feat(models) scoped/all     b12b908 fix(sidebar) archive label
fff941a fix(terminal) PowerShell    502cc16 feat(sidebar) delete side chat
ff8de36 chore(deps) pi 1.5.0        ad1b381 chore(deps) Next 16.4.4
57078bb feat(mobile) touch/Agents   9afc508 feat(settings) mobile levels
1420b81 feat(settings) animation    48a158b fix(agents) row height
1615347 fix(settings) keep section  1fbf6c6 fix(export) remove patch
da6d538 feat(extensions) palette    5e0e47f feat(files) tab drag
f6c8699 feat(models) provider name  8d2fb36 fix(chat) paste
5f31fb6 fix(notifications) dedupe   5644bcf feat(side) tab restore
6386fa4 feat(models) model sorting  79425f8 fix(chat) streaming metrics
797d94b fix(auth) OAuth device ID   7a815d1 feat(chat) switch model while running
7977daf fix(models) signed-out provider
```

## Port status (this round)

Group A + B are implemented (side chats C are not):

- **All of group A**: tab drag reordering, favicon unread badge, quote inserts only the selection
  ("Ask in new chat" kept), Git changes list/tree, prose wrapping by default, HTML preview storage
  shim, mobile touch sizes and Agents button, mobile Settings three-level navigation/system Back/
  animation, model panel sorting, custom message TUI text layout, `AnsiText` palette, streaming
  token/t-s indicator, narrow-screen action buttons and composer icons, paste recovery, notification
  dedupe, PWA manifest credentials, empty sidebar group Pin, and directory-picker/Skills-sort fixes.
- **Group B**: `providerName`/`isScoped`/`?scope=all` on `/api/models`, `providerNames` and display
  names on `/api/auth/providers`, `stalePatterns`/`thinkingPin`/`kind` and `prune` for enabled-models,
  switching models while running (client + existing Rust pass-through), Windows terminal PowerShell
  preference, subagent candidate listing, notification `shownAt` dedupe, and
  `application/manifest+json` for the manifest.
- **Side chats are not recognized**: pi-star neither creates nor recognizes `pi-web:side`; side
  chats created by pi-web are treated as ordinary forks. The creation/interaction UI (`/side`,
  `SessionChatPanel`, side-chat tabs) is not ported.
- **Deferred**: `98ad390` subagent run reconciliation (pi-star has no interactive child sessions, see
  B.4) and `c319c85` tool order (client path; the benefit is prompt caching, see B.5).
- **pi upgrade**: done — `packaging/pi-runtime.json` pins pi 1.6.0. This porting round itself does
  not depend on it: side chats are not implemented, and everything besides `getCurrentTools` uses
  pi-star's own Rust paths.

Local divergences for the next re-sync are in [`ui/UPSTREAM.md`](../ui/UPSTREAM.md).


## pi-web 0.16–0.18 batch 1 (implemented)

The first low-risk batch from 0.16–0.18 is in:

- 100MB single-file uploads (`MAX_UPLOAD_FILE_BYTES` removed; the request total stays 100MB).
- Draft text survives a WebView reload through `pi-star:draft:<session id>` (`lib/draft-store.ts`).
- Project-scoped children drop host secrets and `PORT`/`NODE_ENV`/`NEXT_*` before they run
  (`pi-star-tools` sanitizer; git, npm, git clone and QuickPI shell paths).
- UTF-16 BOM previews, byte-range bounds, quoted empty CSV records, MIME allow-list, streaming
  Markdown reference definitions/fence closes, ANSI escapes in replies, and `path:12-34` links.

A second batch is also in:

- session search now matches every space-separated term (quoted phrases included), returns the
  project name, caches unchanged transcripts by size/mtime, and uses character-safe slices;
- the missing-folder banner can fork the whole session into another existing folder;
- a user message sent during a run gets its entry id before the run ends, so New session works;
- a finished tool result is shown on its card until the canonical tool-result message arrives;
- terminal replays no longer send xterm's replies to the shell, while a new shell still answers
  its startup queries;
- transcript rows far outside the viewport skip layout, and the quote popover keeps its own state,
  so selecting text in long sessions stays fast.

Still open: background subagent report steering, PlantUML and the Agents strip. The selection criteria live in the review notes rather than this 0.13→0.15 document.
