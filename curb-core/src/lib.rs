//! Curb governor core.
//!
//! The environment-agnostic engine: configuration, usage observation, the pure
//! policy state machine, the local enforcement adapter, the read-model service,
//! and the runtime that drives the tick loop. The transport/presentation shell
//! (HTTP API, web embed, CLI, dashboard) lives in the `curb` binary crate and
//! depends on this crate; nothing here references that shell.

pub mod config;
pub mod governor;
pub mod ledger;
pub mod local_enforcer;
pub mod onboarding;
pub mod platform;
pub mod runtime;
pub mod service;
pub mod tail;
pub mod usage;
pub mod usagewatch;
pub mod write_path;
