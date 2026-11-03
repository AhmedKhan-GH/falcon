//! On-device image and text embeddings for falcon's AI Search.
//!
//! The model runs through `candle`, on Metal when it works and on the CPU
//! otherwise. Nothing here reaches the network except [`model::Download`],
//! which only runs when the user asks for it in Preferences; images and
//! queries never leave the Mac.
//!
//! - [`model`] names the pinned model files, downloads and verifies them.
//! - [`embed`] is the [`embed::Embedder`] trait and the preprocessing it uses.
//! - [`clip`] is the CLIP embedder behind that trait.
//! - [`store`] keeps one library's embeddings in falcon's app data, never
//!   inside the `.library` folder.
//! - [`service`] is the worker thread that indexes and answers searches.
//!
//! This crate holds the ML dependencies so the rest of the workspace doesn't
//! rebuild them.

pub mod clip;
pub mod embed;
pub mod model;
pub mod service;
pub mod store;

pub use embed::Embedder;
pub use service::{Phase, Query, SearchError, Service, Status};
pub use store::Hit;
