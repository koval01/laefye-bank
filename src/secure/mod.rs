//! Encrypted protobuf customer channel — raises the bar for browser automation
//! and third-party scripts. TLS still protects transit; this layer obscures
//! request shapes and binds calls to a PoW + ECDH session kept in memory.
//!
//! Public HTTP surface is a single `POST /api` (ApiFrame). Business methods are
//! opaque opcodes inside the encrypted RpcRequest — no path/query on the wire.

mod channel;
mod crypto;
mod dispatch;
mod gate;
mod ops;
mod pow;
mod routes;

pub use channel::ChannelStore;
pub use gate::require_secure_channel;
pub use routes::router;
