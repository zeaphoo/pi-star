//! `session` submodule — mirrors `packages/agent/src/harness/session/*`.
//!
//! Durable session model ([`types`]), the in-memory state reducer ([`state`])
//! + record-log validator ([`reducer`]), the in-memory backend ([`memory`]),
//! the context builder ([`context`]) that projects a branch path to
//! provider-facing messages, and the `Session` + `SessionTree` facade
//! ([`session`]) built over a [`types::SessionStorage`]. The durable pi v3
//! backend lives in `pi-star-core` (`agent::session_store`).
//!
//! Built up incrementally across M5b–M5f.

pub mod context;
pub mod memory;
pub mod reducer;
pub mod session;
pub mod state;
pub mod types;
pub mod values;

pub use context::{
    build_context_entries, build_session_context, default_context_entry_transform,
    session_entry_to_context_messages, ContextEntryTransform, CustomEntryContextMessageProjector,
    SessionContext, SessionContextBuildOptions,
};
pub use memory::{
    Clock, CounterIdGenerator, FakeClock, InMemorySessionRepo, InMemorySessionStorage, SystemClock,
};
pub use reducer::{
    validate_record_log, RecordLogCorruption, RecordLogCorruptionReason, RecordLogSlice,
};
pub use session::{DefaultIdGenerator, Session};
pub use state::{ApplyOutcome, SessionState};
pub use values::{SessionValueWriter, SessionValues, VALUE_CUSTOM_TYPE};
pub mod keyed_queue;
pub mod resources;
pub mod search;
