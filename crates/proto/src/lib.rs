//! Shared protocol, definitions and movement/collision code.
//!
//! This crate is compiled into both the server and the clients (bots today, a
//! real client later), so the wire format, the attribute schema and the
//! collision maths cannot drift apart.

pub mod defs;
pub mod game;
pub mod rng;
pub mod secure;
pub mod snapshot;
pub mod wire;
pub mod world;

pub use game::{Archetype, Behaviour, Game, GameFile};
pub use defs::{ActionDef, AttrDef, Defs, Effect, OnZero};
pub use rng::Rng;
pub use snapshot::{decode_snapshot, encode_snapshot, ClientView, DecodeError, Entry, SnapshotInfo};
pub use world::{Aabb, World};
