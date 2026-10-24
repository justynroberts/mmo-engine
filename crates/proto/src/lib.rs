//! Shared protocol, definitions and movement/collision code.
//!
//! This crate is compiled into both the server and the clients (bots today, a
//! real client later), so the wire format, the attribute schema and the
//! collision maths cannot drift apart.

pub mod blockfeed;
pub mod chat;
pub mod daylight;
pub mod defs;
pub mod game;
pub mod names;
pub mod predict;
pub mod rng;
pub mod secure;
pub mod snapshot;
pub mod tiles;
pub mod wire;
pub mod world;

pub use game::{Archetype, Behaviour, Game, GameFile};
pub use defs::{ActionDef, AttrDef, Defs, Effect, OnZero};
pub use predict::Predictor;
pub use rng::Rng;
pub use snapshot::{decode_snapshot, encode_snapshot, ClientView, DecodeError, Entry, SnapshotInfo};
pub use world::{Aabb, World};
