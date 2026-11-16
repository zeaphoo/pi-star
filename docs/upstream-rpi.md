# Kernel source: pi-rust (rpi)

**English** · [简体中文](upstream-rpi.zh-CN.md)

pi-star's agent kernel is a source copy of [pi-rust](https://github.com/bigfish1913/pi-rust)
(command name `rpi`). **This was a one-time source merge, not a dependency, and no upstream sync
relationship is maintained**: the merged code is maintained by pi-star itself, and later rpi fixes do
not automatically land in this repository.

## Origin

| Item | Value |
| --- | --- |
| Upstream | `https://github.com/bigfish1913/pi-rust` |
| Version | `0.14.0` |
| `commit.id` | `534bcbffb7e1e7551d9ee3572dfeb278e203e493` |
| HEAD at copy time | `0a3593b` |
| License | MIT (including attribution to pi / Mario Zechner) |
| Upstream origin | rpi itself is a Rust port of [`earendil-works/pi`](https://github.com/earendil-works/pi) |

## Mapping

| This repository | Upstream |
| --- | --- |
| `crates/pi-star-ai` | `crates/rpi-ai` |
| `crates/pi-star-agent` | `crates/rpi-agent` |
| `crates/pi-star-tools` | `crates/rpi-tools` |
| `crates/pi-star-harness` | `crates/rpi-harness` |

Upstream crates that were not copied: `rpi-cli`, `rpi-tui`, `rpi-runtime`, `rpi-plugin-sdk`,
`rpi-extensions`, `rpi-telemetry`. pi-star keeps its own config/credentials/model catalog, terminal,
HTTP routes, and extension boundary.

## Local changes

- Crate names and Rust identifiers `rpi_*` → `pi_star_*`; package names become `pi-star-*`.
- Dropped `rpi-telemetry` (none of the four upstream crates actually referenced it).
- `pi-star-tools` no longer depends on `rpi-plugin-sdk`: `SESSION_ID_ENV` / `SUBAGENT_PARENT_ENV`
  became local constants.
- WebP output uses the lossless encoder from `image 0.25` (the upstream lossy API does not exist in
  the current dependency version).
- Compaction entries now use the pi v3 shape: `firstKeptEntryId` / `systemMessage` / `fromHook`,
  dropping upstream's duplicate `retainedTail`; context is selected with pi's
  `buildContextEntries`.
- Added `Entry::Raw` to pass through pi entry types this runtime does not model (`usage`,
  `context_edit`, `label`, `session_info`, `custom_message`, `system` messages), so loading pi
  sessions no longer fails or drops lines on unknown types.
- Removed upstream's `session/jsonl/**` and `session/sqlite.rs` (v4 storage). Persistence is now
  implemented by `pi-star-core::agent::session_store`, which reads/writes pi v3 JSONL directly; the
  harness keeps only the in-memory backend and state machine.

## Maintenance strategy

- Upstream branches are not tracked and there is no rebase. To compare, check out the commit above
  and diff manually.
- Local changes are made directly in the copied code; there is no patch directory.
- Upstream's MIT attribution is kept in the root `NOTICE`; new files do not change that ownership.
