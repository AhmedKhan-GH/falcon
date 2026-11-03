//! AI Models: the providers the user adds in Preferences, their keys, and
//! the one interface every falcon AI feature calls.
//!
//! Eagle's AI plugins share its `ai-sdk`; this crate is falcon's own
//! counterpart, written from the plan in `PLUGINS.md`. It has the same
//! eight provider ids and names models `provider::model`, and it reads
//! nothing of Eagle's.
//!
//! - [`provider`]: the providers, `provider::model` names, the settings
//!   (saved with falcon's preferences; they never hold a key).
//! - [`keys`]: keys in the macOS Keychain, behind [`KeyStore`] so tests use
//!   a double.
//! - [`client`]: generate text, stream text, structured output to a JSON
//!   schema, and image input through messages.
//!
//! Nothing here runs by itself: a request goes out only when a feature
//! calls [`Client`] for a provider the user added, or when the user presses
//! Verify. Every call blocks its caller, so features call it off the UI
//! thread, and it takes a cancel flag and has timeouts. Neither a key nor a
//! request body reaches a log or an error text.

pub mod client;
pub mod keys;
pub mod provider;
pub mod schema;
mod wire;

pub use client::{AiError, BadOutput, Client, Connection, Limits, Message, Part, Request, Role};
pub use keys::{KeyStore, Keychain, MemoryKeys, Secret};
pub use provider::{BaseError, Kind, ModelRef, ProviderId, ProviderSettings, Settings};
