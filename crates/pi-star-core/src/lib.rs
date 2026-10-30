//! Domain layer of pi-star: pi-compatible session files, settings, workspace
//! files, Git and the agent runtime. It intentionally has no HTTP dependency so
//! it can also be embedded in the Tauri shell.

pub mod agent;
pub mod config;
pub mod file_access;
pub mod files;
pub mod git;
pub mod models;
pub mod project;
pub mod push;
pub mod session;
pub mod session_index;
pub mod settings;
pub mod terminal;
pub mod transcript_notices;
pub mod ui_state;

pub use config::Config;
