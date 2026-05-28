//! Core Curb modules.
//!
//! The Rust rewrite keeps the same strategic shape as the launch design:
//! deep modules own parsing, safety, policy, and persistence; clients compose
//! those modules instead of reimplementing their internals.

pub mod api;
pub mod config;
pub mod ledger;
pub mod platform;
pub mod runtime;
pub mod service;
pub mod usage;
