// `async_trait` generates `#[must_use]` futures whose `Result` outputs are
// already `#[must_use]`; Rust 1.99's Clippy reports the generated overlap.
#![allow(clippy::double_must_use)]

//! # Context Harness Core
//!
//! Shared, WASM-safe logic for Context Harness: data models, chunking,
//! store abstraction, search algorithm, and embedding trait.
//!
//! This crate contains no tokio, sqlx, filesystem I/O, or other
//! native-only dependencies. It compiles to both native targets and
//! `wasm32-unknown-unknown`.

pub mod chunk;
pub mod embedding;
pub mod models;
pub mod search;
pub mod store;
