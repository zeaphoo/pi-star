# Sub-agents

A subagent is a real Pi session created for a focused task and linked to the
session that started it. Both agent engines create them, and both write the same
custom entries, so the sidebar, the Agents panel and the trace view do not care
which engine ran a child.

## Contract

Every child session starts with:

```json
{ "type": "custom", "customType": "pi-star:subagent",
  "data": { "version": 1, "parentSessionId": "…", "profile": "explore",
            "description": "…", "task": "…", "runInBackground": true } }
```

and one entry per transition:

```json
{ "customType": "pi-star:subagent-status", "data": { "status": "queued" } }
{ "customType": "pi-star:subagent-result",
  "data": { "status": "completed", "result": "…", "completedAt": "…" } }
```

Statuses: `queued → starting → running → completed | failed | aborted |
interrupted`. A persisted `running`/`queued` with no live run reads as
`interrupted`, so a restart never leaves a spinner that cannot end.

The tool result that started the run carries
`details.kind = "pi-star-subagent"` with the session id, profile, description,
status and background flag. The tool card's "open" button and the Agents panel
read only that.

## Tools

| Tool | Purpose |
| --- | --- |
| `Agent` (RPC) / `subagent` (QuickPI) | Start a run: profile, task, description, `run_in_background`, `wait_timeout`, `model` |
| `get_subagent_result` | Read the latest result; `wait: true` waits for completion |
| `steer_subagent` | Send a steering message into a running child |

Tools register only while Settings › Sub-agents → **Built-in subagents** is on,
and are active on registration so the model can use them without switching them
on first. Toggling needs a session reload: the RPC backend reloads its extension,
and QuickPI rebuilds the runtime so its tool registry is rebuilt too.

Project and workspace profiles are the project's own prompt and tool grant, so
they load only while the folder is trusted — the same rule pi applies to `.pi/`
resources. The Settings panel still lists the files.

## Background reports

A background run's report is not a user message. It carries a prefix saying so,
because pi replays custom messages as ordinary user turns and compaction would
otherwise file the report under the user's goals.

Delivery has two paths:

- **Parent running** — at a `turn_end` whose queue is empty and whose reply did
  not end in an error or an abort, the report is sent with `deliverAs: "steer"`,
  so the parent's next model call in the same run sees it.
- **Parent idle** — the report is sent as a `followUp` with `triggerTurn: true`.

A result the parent already collected with `get_subagent_result` is marked
consumed and is never delivered again. Stop drops a steer that never reached the
model; the report then wakes the idle parent instead.

## Control

A subagent is part of the agent that started it: there is no independent
composer, no separate stop button and no resume. Stopping the parent stops its
subagents too, and a subagent's session is only ever read back for inspection.
Worktree isolation is likewise not offered; a child runs in its parent's folder.

## Profile resources

| Profile field | Standard PI (RPC) | QuickPI |
| --- | --- | --- |
| `prompt_mode: replace \| append` | `--system-prompt` / `--append-system-prompt` | replaces / appends the system prompt |
| `load_skills`, `skills: [...]` | `--no-skills`; a list preloads bodies | hides the catalog / preloads bodies |
| `load_extensions`, `extensions: [...]` | `--no-extensions`, explicit paths or `builtin:<name>` | **not supported** (no TS extension host) |
| `codemode: on \| inherit \| off` | `-e builtin:codemode` plus the `codemode` tool | **not supported** (no Code mode tool); `on` is refused |
| `mcp: on \| off` | `--no-mcp` when off | the MCP bridge is skipped when off |
| `inherit_context` | parent's recent messages prepended to the task (12k characters) | same |

Defaults match pi-web: skills, extensions, MCP and Code mode are **off** unless
the profile asks for them. QuickPI refuses a profile that requires extensions or
Code mode with an explicit error instead of running without them; switch the
session to the standard PI engine for those.

## Limits

- `max_turns` from the profile: one wrap-up turn, then the run fails with a
  turn-limit error carrying any partial output.
- Concurrency: `maxConcurrent` (default 10, maximum 32), **per parent session**:
  one busy project never blocks another's subagents. Runs beyond the limit are
  queued rather than refused, and a queued run's session file already exists and
  says `queued`.
- `run_in_background` falls back to the profile's own value when the call does
  not name it.
- A QuickPI child is written with `pi-star:agent-backend = quick`, so opening it
  later uses the engine that created it.

## Viewing a run

The sub-agent transcript dialog has two views:

- **Transcript** — the child's messages, polled from `/api/sessions/{id}/context`.
- **Trace** — the same dense event list as the main Trace tab, read from
  `/api/sessions/{id}/entries`.

Both are read-only and never open an agent event stream, so viewing a run cannot
resume, disturb or resurrect it. The dialog opens from the Agent tool card, the
completion report card in the transcript, or the Agents list; Escape, the
backdrop, and its close button close it.
