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
