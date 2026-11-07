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
