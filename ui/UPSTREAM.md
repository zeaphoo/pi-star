# UI upstream

Copied from the read-only pi-web checkout at `/home/ubuntu/workspace/pi-star/pi-web`.

- Source project: https://github.com/agegr/pi-web (MIT)
- Upstream version: 0.15.0 (client modules re-synced from v0.15.0; see
  [`docs/upstream-pi-web.md`](../docs/upstream-pi-web.md))
- Copied client modules: 232 of v0.15.0's 235 (231 at the original v0.13.0 baseline);
  `SessionChatPanel.tsx`, `side-tab-state.ts` and `side-chat-client.ts` are the side-chat
  modules left out on purpose
- Re-copy: `node scripts/sync-ui-from-pi-web.mjs`
- Do not copy `app/api/**` or server-only `lib/**` modules; those are implemented in Rust.
- Tests for those server modules are not copied; Rust unit/integration tests cover them.

## Local divergences from upstream

These are intentional and must be re-applied after a re-sync:

- **Side chats (`/side`) are not ported.** Upstream v0.15's `SessionChatPanel`, `side-tab-state`,
  `side-chat-client`, `session-family` side helpers, and the side-chat branches in `AppShell`,
  `ChatWindow`, `ChatInput`, `useAgentSession`, `AgentSessionPanel`, `SessionSidebar`, `TabBar`
  and the locale files stay out. Subagent tabs still use `SubagentViewer`, and the
  selected-text popover keeps "Ask in new chat".
- `next/*`, `process.env.NEXT_PUBLIC_*` and the React/Vite adaptations are applied by
  `scripts/sync-ui-from-pi-web.mjs` (`postProcess`); `src/main.tsx`, `src/shims/`, `src/types/`,
  `src/lib/desktop.ts` and `src/components/AgentBackendSelector.tsx` are pi-star additions.
- Branding and the version line ("Pi Star", `VITE_APP_VERSION`, `VITE_PI_VERSION`) replace
  upstream's "Pi Web" and `NEXT_PUBLIC_*` reads.
- The sidebar brand is pi-star's own (`PiWebTitle` without the version popover,
  `sidebar-brand-logo` in `app/sidebar.css`), not upstream's click-for-versions brand.
- pi-web side chats are recognized (`SessionInfo.relation.kind === "side"`) and folded into
  their parent's family but are not shown anywhere (no `/side`, no side-chat panel); the
  Agents panel and badge list only subagents. pi-star never creates side chats, only hides
  the ones pi-web wrote.
- `ChatWindow` places `ChatMinimap` over the whole chat column (absolute, top to bottom)
  with a 36px spacer in the message row, and passes `rightInset` to `ExtensionStatusBar`,
  so the rail reaches the page bottom; upstream ends it where the composer starts.
- The sidebar header's New is a split control: the button starts a session in the current
  project (or opens the folder picker when there is none, instead of sitting disabled), and
  the caret opens a menu with the folder picker, the projects used last and the default
  directory. `sidebar.newMenuTitle`, `sidebar.newSession`, `sidebar.openProject`,
  `sidebar.openProjectTitle`, `sidebar.openOrCreateProject` and `sidebar.recentProjects`
  are pi-star additions in all three locales.
- The session tree's footer keeps only the archive link; "Open another project…" moved to
  the header's New menu (a split button: new session in the current project, or the folder
  picker and the projects used last). The Code view's tree header carries the files' upload,
  ignored-files and search keys that the retired sidebar files panel had.
- Settings › General is a split view with two rows, **Appearance** (theme, fonts
  & layout, language, version) and **Chat** (thinking expansion, quote selection,
  send key, cache/cost notices, the Windows shell tool), instead of one long
  page. On a phone it follows the same list → detail levels as the other
  sections.
- Upstream's toolbar "System" and "Tools" panels are gone: both read the same
  session information and sat next to each other, so they became one read-only
  Settings section, **Session context** (`components/SessionContextSettings.tsx`,
  `settings.context`), with a refresh button. The chat still owns the request
  (`onSystemInfoLoaderChange`); Settings only asks it to run.
- A session-scoped artifacts pane (`components/SessionArtifactsPane.tsx`,
  `lib/session-artifacts.ts`) sits to the right of `ChatWindow`, toggled beside Debug, with
  per-session file tabs and a remembered open state. Chat links and written-file chips open
  there, and a sub-agent's transcript opens there too (`file-tab-state.ts`'s `openAgentTab`),
  so the pane is the session's whole right side. The whole-session artifact list is paged
  from the context endpoint, outside the chat's own message window. The pane floats over the
  chat (`is-overlay`, decided by the room left after the sidebar and the dock) instead of
  squeezing it.
- Upstream's shared right panel (`#file-panel`, `rightPanelOpen`, per-workspace parked file
  tabs, panel terminals and sub-agent tabs) is replaced by a **right dock**
  (`components/RightDock.tsx`, `DockNodeView.tsx`, `lib/dock-state.ts`): a project-scoped
  split tree of independent Code/Diff/Terminal leaves. Every leaf keeps its own tabs,
  selection and shells — nothing is shared with the main view, and a dock terminal is a
  different PTY from the main view's. The layout and each leaf's state are parked per
  project (`pi-star:dock:v1`); the dock's width and open state are the user's own
  preferences (`pi-star:dock-width`, `pi-star:dock-open`). It is offered at ≥1024px only,
  capped at four leaves (two of them terminals). The split-tree helpers it shares with the
  terminal workspace live in `lib/split-tree.ts`.
