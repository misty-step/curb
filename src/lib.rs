//! Core Curb modules.
//!
//! The Rust implementation keeps the same strategic shape as the launch design:
//! deep modules own parsing, safety, policy, and persistence; clients compose
//! those modules instead of reimplementing their internals.

pub mod api;
pub mod cli;
pub mod config;
pub mod dashboard;
pub mod http;
pub mod ledger;
pub mod local_enforcer;
pub mod onboarding;
pub mod platform;
pub mod runtime;
pub mod service;
pub mod tail;
pub mod usage;
pub mod usagewatch;
pub mod web;
pub mod write_path;
