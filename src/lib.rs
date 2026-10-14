//! Daily AI briefing for ZECTRIX Hub.
//!
//! The library holds the pieces a third-party app is likely to reuse or
//! replace: feed parsing, clustering, ranking, the LLM trait, storage, the
//! e-ink card layout, and the Hub client. `server` is the process that wires
//! them to a loopback settings page and the device event loop.

#![forbid(unsafe_code)]

pub mod cluster;
pub mod config;
pub mod device;
pub mod fetch;
pub mod hub;
pub mod i18n;
pub mod layout;
pub mod llm;
pub mod model;
pub mod parse;
pub mod pipeline;
pub mod rank;
pub mod server;
pub mod speech;
pub mod store;
pub mod text;

pub use model::{Article, Cluster, Digest, DigestItem};
