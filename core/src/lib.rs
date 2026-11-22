//! CloudStream for Windows — core logic.
//!
//! Independent implementation inspired by https://github.com/recloudstream/cloudstream (GPL-3.0).
//! No CloudStream source is copied. Repository data is treated as untrusted *data*; nothing
//! downloaded from a repository is ever executed.

pub mod api;
pub mod archive;
pub mod compat;
pub mod ffi;
pub mod http;
pub mod models;
pub mod provider;
pub mod repo;
pub mod store;
