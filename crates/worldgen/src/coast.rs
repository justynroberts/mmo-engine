//! A coastline: land inside one or more outlines, sea outside. Everything that asks "how far inland is this?" (the
//! hills, where a town may stand, where a road may go, where the sea floor is too deep to wade) reads one signed
//! distance, sampled once on a grid and interpolated, so a long coast costs nothing per question.

/// Grid spacing of the distance samples, in world units.
const CELL: f32 = 8.0;
/// How far past the world's edge the grid runs (the sea and the hills beyond are drawn there too).
const MARGIN: f32 = 256.0;
/// Distances are not worked out beyond this; past it the land is simply "far inland" or "far out to sea".
const CAP: f32 = 400.0;

pub struct Coast {
    x0: f32,
    n: usize,
    d: Vec<f32>,
}

impl Coast {
    pub fn new(size: f32, outlines: Vec<Vec<(f32, f32)>>) -> Coast {
        let x0 = -MARGIN;
        let n = ((size + 2.0 * MARGIN) / CELL).ceil() as usize + 1;
        let segs: Vec<((f32, f32), (f32, f32))> = outlines.iter().flat_map(|o| (0..o.len()).map(move |k| (o[k], o[(k + 1) % o.len()]))).collect();
        // Segments by 64 unit bucket, padded by the cap, so each sample checks only the coast near it.
        let b = 64.0;
        let nb = ((size + 2.0 * MARGIN) / b).ceil() as i32 + 1;
        let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); (nb * nb) as usize];
        for (i, &(p, q)) in segs.iter().enumerate() {
            let bx = |v: f32| (((v - x0) / b).floor() as i32).clamp(0, nb - 1);
            let (i0, i1) = (bx(p.0.min(q.0) - CAP), bx(p.0.max(q.0) + CAP));
            let (j0, j1) = (bx(p.1.min(q.1) - CAP), bx(p.1.max(q.1) + CAP));
            for j in j0..=j1 {
                for k in i0..=i1 {
                    buckets[(j * nb + k) as usize].push(i as u32);
                }
            }
        }
        let mut d = vec![0.0; n * n];
        for j in 0..n {
            for i in 0..n {
                let (x, y) = (x0 + i as f32 * CELL, x0 + j as f32 * CELL);
                let bi = (((x - x0) / b).floor() as i32).clamp(0, nb - 1);
                let bj = (((y - x0) / b).floor() as i32).clamp(0, nb - 1);
                let mut best = CAP;
                for &s in &buckets[(bj * nb + bi) as usize] {
                    let (p, q) = segs[s as usize];
                    let (vx, vy) = (q.0 - p.0, q.1 - p.1);
                    let t = (((x - p.0) * vx + (y - p.1) * vy) / (vx * vx + vy * vy).max(1e-6)).clamp(0.0, 1.0);
                    best = best.min((p.0 + vx * t - x).hypot(p.1 + vy * t - y));
                }
                d[j * n + i] = if inside(&outlines, x, y) { best } else { -best };
            }
        }
        Coast { x0, n, d }
    }

    /// How far inland a point is: positive on land, negative at sea, 0 on the shore.
    pub fn d(&self, x: f32, y: f32) -> f32 {
        let fx = ((x - self.x0) / CELL).clamp(0.0, (self.n - 1) as f32 - 1e-3);
        let fy = ((y - self.x0) / CELL).clamp(0.0, (self.n - 1) as f32 - 1e-3);
        let (i, j) = (fx as usize, fy as usize);
        let (tx, ty) = (fx - i as f32, fy - j as f32);
        let at = |i: usize, j: usize| self.d[j * self.n + i];
        let top = at(i, j) + (at(i + 1, j) - at(i, j)) * tx;
        let bot = at(i, j + 1) + (at(i + 1, j + 1) - at(i, j + 1)) * tx;
        top + (bot - top) * ty
    }
}

/// Even-odd point in polygon, over every outline (an island inside a lake inside land works out).
fn inside(outlines: &[Vec<(f32, f32)>], x: f32, y: f32) -> bool {
    let mut c = false;
    for o in outlines {
        let mut j = o.len() - 1;
        for i in 0..o.len() {
            let (a, b) = (o[i], o[j]);
            if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
                c = !c;
            }
            j = i;
        }
    }
    c
}

/// The open sea as solid boxes (so nobody walks the sea floor): every `cell` square further than `shallows` from the
/// shore, inside the world, merged along rows and then down columns of identical rows.
pub fn sea_boxes(c: &Coast, size: f32, shallows: f32) -> Vec<[f32; 4]> {
    let cell = 16.0;
    let n = (size / cell).ceil() as usize;
    let mut rows: Vec<Vec<(usize, usize)>> = Vec::with_capacity(n);
    for j in 0..n {
        let mut runs = Vec::new();
        let mut start = None;
        for i in 0..=n {
            let deep = i < n && {
                let (x, y) = ((i as f32 + 0.5) * cell, (j as f32 + 0.5) * cell);
                // A cell counts as deep only if all of it is: its middle at least half a diagonal past the shallows.
                c.d(x, y) < -(shallows + cell * 0.71)
            };
            match (deep, start) {
                (true, None) => start = Some(i),
                (false, Some(s)) => {
                    runs.push((s, i));
                    start = None;
                }
                _ => {}
            }
        }
        rows.push(runs);
    }
    let mut out = Vec::new();
    let mut open: Vec<((usize, usize), usize)> = Vec::new(); // run, first row
    for (j, runs) in rows.iter().enumerate().chain(std::iter::once((n, &Vec::new()))) {
        let mut next = Vec::new();
        for &(run, from) in &open {
            if runs.contains(&run) {
                next.push((run, from));
            } else {
                let b = [run.0 as f32 * cell, from as f32 * cell, (run.1 as f32 * cell).min(size), (j as f32 * cell).min(size)];
                out.push(b);
            }
        }
        for &run in runs {
            if !next.iter().any(|(r, _)| *r == run) {
                next.push((run, j));
            }
        }
        open = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inland_is_positive_the_sea_negative_and_the_sea_is_boxed_off_past_the_shallows() {
        let c = Coast::new(512.0, vec![vec![(100.0, 100.0), (400.0, 100.0), (400.0, 400.0), (100.0, 400.0)]]);
        assert!((c.d(250.0, 250.0) - 150.0).abs() < 5.0, "the middle, where the samples round the peak off a little");
        assert!((c.d(100.0, 250.0)).abs() < 1.0);
        assert!((c.d(50.0, 250.0) + 50.0).abs() < 1.0);
        let boxes = sea_boxes(&c, 512.0, 8.0);
        assert!(!boxes.is_empty());
        for b in &boxes {
            for (x, y) in [(b[0], b[1]), (b[2], b[3]), (b[0], b[3]), (b[2], b[1])] {
                assert!(c.d(x, y) < -8.0 + 1.0, "a sea box reaches the shallows at ({x}, {y})");
            }
        }
        assert!(boxes.len() < 40, "merged into a few boxes, not {}", boxes.len());
    }
}
