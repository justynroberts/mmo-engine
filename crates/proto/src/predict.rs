//! Client-side prediction and reconciliation.
//!
//! The client applies its own input immediately with the *same* `integrate`
//! function the server runs, so movement feels instant. Each snapshot carries
//! the newest input sequence the server has applied; the client then snaps to
//! the server's position and replays the inputs the server has not seen yet.
//! When nothing unexpected happened the correction is zero (modulo the 1/16
//! unit position quantisation); a large correction means the server did
//! something the client could not know about, such as a respawn.

use std::collections::VecDeque;

use crate::world::{integrate, World, GROUND};

/// Inputs kept for replay. At 20 Hz this is over three seconds of unacked input.
const MAX_PENDING: usize = 64;

pub struct Predictor {
    pub x: f32,
    pub y: f32,
    /// The layer the feet are in (`world::GROUND` on the bare ground).
    pub z: i8,
    speed: f32,
    dt: f32,
    radius: f32,
    pending: VecDeque<(u32, i8, i8)>,
}

impl Predictor {
    pub fn new(x: f32, y: f32, speed: f32, hz: u32, radius: f32) -> Predictor {
        Predictor { x, y, z: GROUND, speed, dt: 1.0 / hz as f32, radius, pending: VecDeque::new() }
    }

    /// A haste or a slow (`game::SPEED_ATTR`): the speed steps are predicted at from now on.
    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
    }

    /// Predict one tick of movement for an input that is also being sent to the server.
    pub fn apply_input(&mut self, world: &World, seq: u32, dir_x: i8, dir_y: i8) {
        (self.x, self.y, self.z) = integrate(world, self.x, self.y, self.z, dir_x, dir_y, self.speed, self.dt, self.radius);
        if self.pending.len() == MAX_PENDING {
            self.pending.pop_front();
        }
        self.pending.push_back((seq, dir_x, dir_y));
    }

    /// Adopt the server's position and replay what it has not processed yet.
    /// Returns how far the corrected position is from what we had predicted (sideways; layers are not counted).
    pub fn reconcile(&mut self, world: &World, server_x: f32, server_y: f32, server_z: i8, last_input_seq: u32) -> f32 {
        let (px, py) = (self.x, self.y);
        // Wrapping compare so sequence overflow is harmless.
        while let Some(&(seq, _, _)) = self.pending.front() {
            if (last_input_seq.wrapping_sub(seq) as i32) >= 0 {
                self.pending.pop_front();
            } else {
                break;
            }
        }
        self.x = server_x;
        self.y = server_y;
        self.z = server_z;
        for &(_, dx, dy) in &self.pending {
            (self.x, self.y, self.z) = integrate(world, self.x, self.y, self.z, dx, dy, self.speed, self.dt, self.radius);
        }
        ((self.x - px).powi(2) + (self.y - py).powi(2)).sqrt()
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;
    use crate::world::{dpos, qpos, Aabb};

    fn walls() -> World {
        World::from_boxes(
            256.0,
            1,
            vec![Aabb { x0: 100.0, y0: 0.0, x1: 110.0, y1: 200.0 }, Aabb { x0: 0.0, y0: 120.0, x1: 90.0, y1: 130.0 }],
        )
    }

    #[test]
    fn prediction_matches_a_lockstep_server_exactly() {
        let w = World::generate(512, 9);
        let mut rng = Rng::new(5);
        let (sx, sy) = w.random_spawn(&mut rng, 0.5);
        let mut w = w;
        // Some built blocks around the spawn, in steps and towers, so climbing and dropping are predicted too.
        let (bx, by) = ((sx / 2.0) as u32, (sy / 2.0) as u32);
        for i in 0..40u32 {
            let (cx, cy) = (bx + rng.below(12) - 6, by + rng.below(12) - 6);
            w.set_column(cx, cy, crate::world::Column { base: (i % 3) as i8 - 1, mask: (1u16 << (1 + rng.below(4))) - 1 });
        }
        let mut server = (sx, sy, GROUND);
        let mut p = Predictor::new(sx, sy, 20.0, 20, 0.5);
        for seq in 1..=3000u32 {
            let d = [-127i8, 0, 127];
            let (dx, dy) = (d[rng.below(3) as usize], d[rng.below(3) as usize]);
            p.apply_input(&w, seq, dx, dy);
            server = integrate(&w, server.0, server.1, server.2, dx, dy, 20.0, 0.05, 0.5);
            assert_eq!((p.x, p.y, p.z), server, "diverged at input {seq}");
            // The server acks, quantised on the wire.
            let err = p.reconcile(&w, dpos(qpos(server.0)), dpos(qpos(server.1)), server.2, seq);
            assert!(err <= 0.05, "correction {err} at input {seq}");
            server = (p.x, p.y, p.z); // both sides continue from the quantised position
        }
    }

    #[test]
    fn replays_unacked_inputs_after_the_server_position() {
        let w = walls();
        let mut p = Predictor::new(50.0, 50.0, 20.0, 20, 0.5);
        for seq in 1..=10 {
            p.apply_input(&w, seq, 127, 0);
        }
        let before = (p.x, p.y);
        // Server has only processed 6 of them (at 50 + 6 units).
        let err = p.reconcile(&w, 56.0, 50.0, GROUND, 6);
        assert_eq!(p.pending(), 4);
        assert!(err < 1e-4, "{err}");
        assert_eq!((p.x, p.y), before);
    }

    #[test]
    fn a_server_side_teleport_shows_up_as_a_large_correction() {
        let w = walls();
        let mut p = Predictor::new(50.0, 50.0, 20.0, 20, 0.5);
        p.apply_input(&w, 1, 127, 0);
        let err = p.reconcile(&w, 200.0, 200.0, GROUND, 1);
        assert!(err > 100.0);
        assert_eq!((p.x, p.y), (200.0, 200.0));
    }

    #[test]
    fn walls_stop_the_predicted_entity_like_the_server() {
        let w = walls();
        let mut p = Predictor::new(95.0, 50.0, 20.0, 20, 0.5);
        for seq in 1..=100 {
            p.apply_input(&w, seq, 127, 0);
        }
        assert!(p.x < 100.0, "tunnelled: {}", p.x);
    }

    #[test]
    fn sequence_wraparound_is_handled() {
        let w = walls();
        let mut p = Predictor::new(10.0, 10.0, 20.0, 20, 0.5);
        for seq in [u32::MAX - 1, u32::MAX, 0, 1] {
            p.apply_input(&w, seq, 0, 0);
        }
        p.reconcile(&w, 10.0, 10.0, GROUND, u32::MAX);
        assert_eq!(p.pending(), 2);
    }

    #[test]
    fn pending_is_bounded() {
        let w = walls();
        let mut p = Predictor::new(10.0, 10.0, 20.0, 20, 0.5);
        for seq in 0..1000 {
            p.apply_input(&w, seq, 0, 0);
        }
        assert_eq!(p.pending(), MAX_PENDING);
    }
}
