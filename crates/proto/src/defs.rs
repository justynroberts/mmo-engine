//! Definitions: the data that makes this a game rather than an engine.
//!
//! In the full plan these come from a Postgres DB via a baker that emits a
//! versioned blob. For the spike they are built in code, but everything the
//! server does with them is already data driven: attributes, regen, actions
//! and effects. Health is just attribute 0.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnZero {
    None,
    /// Reset all attributes to defaults and move to a fresh spawn point.
    Respawn,
}

#[derive(Clone, Debug)]
pub struct AttrDef {
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    /// Quantisation width on the wire (8 or 16 in this byte-aligned spike).
    pub bits: u8,
    pub replicated: bool,
    pub regen_per_sec: f32,
    pub on_zero: OnZero,
}

#[derive(Clone, Copy, Debug)]
pub enum Effect {
    AddToTarget { attr: u8, delta: f32 },
    AddToSelf { attr: u8, delta: f32 },
}

#[derive(Clone, Debug)]
pub struct ActionDef {
    pub id: u8,
    pub name: String,
    pub cooldown_ticks: u16,
    pub range: f32,
    /// Self attribute must be at least this value to use the action.
    pub requires: Option<(u8, f32)>,
    pub effects: Vec<Effect>,
}

#[derive(Clone, Debug)]
pub struct Defs {
    pub attrs: Vec<AttrDef>,
    pub actions: Vec<ActionDef>,
    /// Indices into `attrs` of replicated attributes, in wire order.
    pub replicated: Vec<usize>,
    pub move_speed: f32,
    pub entity_radius: f32,
    pub interest_radius: f32,
    pub max_visible: usize,
}

pub const ATTR_HEALTH: u8 = 0;
pub const ATTR_ENERGY: u8 = 1;
pub const ACTION_ZAP: u8 = 1;

impl Defs {
    pub fn default_game() -> Defs {
        let attrs = vec![
            AttrDef {
                name: "health".into(),
                min: 0.0,
                max: 100.0,
                default: 100.0,
                bits: 8,
                replicated: true,
                regen_per_sec: 0.0,
                on_zero: OnZero::Respawn,
            },
            AttrDef {
                name: "energy".into(),
                min: 0.0,
                max: 100.0,
                default: 100.0,
                bits: 8,
                replicated: false,
                regen_per_sec: 5.0,
                on_zero: OnZero::None,
            },
        ];
        let actions = vec![ActionDef {
            id: ACTION_ZAP,
            name: "zap".into(),
            cooldown_ticks: 10,
            range: 8.0,
            requires: Some((ATTR_ENERGY, 5.0)),
            effects: vec![
                Effect::AddToTarget { attr: ATTR_HEALTH, delta: -10.0 },
                Effect::AddToSelf { attr: ATTR_ENERGY, delta: -5.0 },
            ],
        }];
        let replicated = attrs
            .iter()
            .enumerate()
            .filter(|(_, a)| a.replicated)
            .map(|(i, _)| i)
            .collect();
        Defs {
            attrs,
            actions,
            replicated,
            move_speed: 20.0,
            entity_radius: 0.5,
            interest_radius: 128.0,
            max_visible: 64,
        }
    }

    pub fn action(&self, id: u8) -> Option<&ActionDef> {
        self.actions.iter().find(|a| a.id == id)
    }

    /// Bytes one replicated attribute takes on the wire.
    #[inline]
    pub fn attr_wire_bytes(&self, attr: usize) -> usize {
        if self.attrs[attr].bits <= 8 {
            1
        } else {
            2
        }
    }

    /// Quantise a float attribute value into its wire integer.
    #[inline]
    pub fn quantize(&self, attr: usize, v: f32) -> u16 {
        let a = &self.attrs[attr];
        let levels = ((1u32 << a.bits.min(16)) - 1) as f32;
        let t = ((v - a.min) / (a.max - a.min)).clamp(0.0, 1.0);
        (t * levels).round() as u16
    }

    #[inline]
    pub fn dequantize(&self, attr: usize, q: u16) -> f32 {
        let a = &self.attrs[attr];
        let levels = ((1u32 << a.bits.min(16)) - 1) as f32;
        a.min + (q as f32 / levels) * (a.max - a.min)
    }

    /// FNV-1a over a canonical serialisation. Sent in the handshake so a client
    /// built against different definitions is rejected rather than misparsing.
    pub fn content_hash(&self) -> u64 {
        let mut h = Fnv::new();
        for a in &self.attrs {
            h.bytes(a.name.as_bytes());
            h.f32(a.min);
            h.f32(a.max);
            h.f32(a.default);
            h.bytes(&[a.bits, a.replicated as u8, a.on_zero as u8]);
            h.f32(a.regen_per_sec);
        }
        for a in &self.actions {
            h.bytes(&[a.id]);
            h.bytes(a.name.as_bytes());
            h.bytes(&a.cooldown_ticks.to_le_bytes());
            h.f32(a.range);
            if let Some((attr, v)) = a.requires {
                h.bytes(&[1, attr]);
                h.f32(v);
            }
            for e in &a.effects {
                match *e {
                    Effect::AddToTarget { attr, delta } => {
                        h.bytes(&[10, attr]);
                        h.f32(delta);
                    }
                    Effect::AddToSelf { attr, delta } => {
                        h.bytes(&[11, attr]);
                        h.f32(delta);
                    }
                }
            }
        }
        h.f32(self.move_speed);
        h.f32(self.entity_radius);
        h.f32(self.interest_radius);
        h.bytes(&(self.max_visible as u32).to_le_bytes());
        h.0
    }
}

struct Fnv(u64);
impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
    fn bytes(&mut self, b: &[u8]) {
        for &x in b {
            self.0 ^= x as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn f32(&mut self, v: f32) {
        self.bytes(&v.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_and_sensitive() {
        let a = Defs::default_game();
        let b = Defs::default_game();
        assert_eq!(a.content_hash(), b.content_hash());
        let mut c = Defs::default_game();
        c.move_speed += 1.0;
        assert_ne!(a.content_hash(), c.content_hash());
        let mut d = Defs::default_game();
        d.actions[0].effects[0] = Effect::AddToTarget { attr: ATTR_HEALTH, delta: -11.0 };
        assert_ne!(a.content_hash(), d.content_hash());
    }

    #[test]
    fn quantize_roundtrip_is_close() {
        let d = Defs::default_game();
        for v in [0.0f32, 1.0, 37.5, 99.0, 100.0] {
            let q = d.quantize(0, v);
            let back = d.dequantize(0, q);
            assert!((back - v).abs() <= 100.0 / 255.0, "{v} -> {back}");
        }
    }
}
