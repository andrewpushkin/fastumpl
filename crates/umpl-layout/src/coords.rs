//! Turns ordered layers into coordinates.
//!
//! Each layer is placed by solving a small least-squares problem: every box
//! wants its centre at the weighted mean of its neighbours' centres in the
//! next layer, subject to keeping its order and minimum gaps. That problem
//! is solved exactly by pool-adjacent-violators. Sweeping down and up a few
//! times centres parents over their children and straightens long lines.

use std::collections::HashMap;

use crate::graph::{LAYER_GAP, NODE_GAP};
use crate::ordering::Layered;

/// Weighted isotonic regression: the nondecreasing `y` closest to `t`.
fn pav(t: &[f64], w: &[f64]) -> Vec<f64> {
    // Blocks of (mean, weight, count).
    let mut blocks: Vec<(f64, f64, usize)> = Vec::with_capacity(t.len());
    for i in 0..t.len() {
        blocks.push((t[i], w[i], 1));
        while blocks.len() >= 2 {
            let n = blocks.len();
            if blocks[n - 2].0 <= blocks[n - 1].0 {
                break;
            }
            let b = blocks.pop().unwrap();
            let a = blocks.last_mut().unwrap();
            let weight = a.1 + b.1;
            a.0 = (a.0 * a.1 + b.0 * b.1) / weight;
            a.1 = weight;
            a.2 += b.2;
        }
    }
    let mut out = Vec::with_capacity(t.len());
    for (mean, _, count) in blocks {
        out.extend(std::iter::repeat_n(mean, count));
    }
    out
}

/// Minimum distance between the centres of neighbours `a` (left) and `b`.
fn separation(lay: &Layered, widths: &[i32], a: usize, b: usize) -> f64 {
    let gap = match (lay.is_dummy(a), lay.is_dummy(b)) {
        (false, false) => NODE_GAP,
        (true, true) => 2,
        _ => 3,
    };
    (widths[a] + widths[b]) as f64 / 2.0 + gap as f64
}

/// Places one layer as close as possible to the desired centres.
fn place_layer(lay: &Layered, widths: &[i32], layer: &[usize], desired: &[f64], weight: &[f64]) -> Vec<f64> {
    // c_i = y_i + off_i with off_i the packed offsets turns the gap
    // constraints into y nondecreasing.
    let mut off = vec![0.0; layer.len()];
    for i in 1..layer.len() {
        off[i] = off[i - 1] + separation(lay, widths, layer[i - 1], layer[i]);
    }
    let t: Vec<f64> = desired.iter().zip(&off).map(|(d, o)| d - o).collect();
    let y = pav(&t, weight);
    y.iter().zip(&off).map(|(y, o)| y + o).collect()
}

fn link_weights(lay: &Layered) -> HashMap<(usize, usize), f64> {
    let mut m = HashMap::new();
    for &(u, l, w) in &lay.links {
        *m.entry((u, l)).or_insert(0.0) += w;
        *m.entry((l, u)).or_insert(0.0) += w;
    }
    m
}

/// Returns the top-left corner of every real item.
pub(crate) fn assign_coords(lay: &Layered, widths: &[i32], heights: &[i32]) -> Vec<(i32, i32)> {
    let total = lay.total();
    let mut center = vec![0.0f64; total];
    // Start packed from the left.
    for layer in &lay.layers {
        let mut x = 0.0;
        for (p, &i) in layer.iter().enumerate() {
            if p > 0 {
                x += separation(lay, widths, layer[p - 1], i);
            }
            center[i] = x;
        }
    }
    let n_layers = lay.layers.len();
    let weights = link_weights(lay);
    for it in 0..12 {
        // 0 = use neighbours above, 1 = below, 2 = both.
        let mode = if it >= 10 { 2 } else { it % 2 };
        let ks: Vec<usize> = if mode == 1 { (0..n_layers).rev().collect() } else { (0..n_layers).collect() };
        for k in ks {
            let layer = &lay.layers[k];
            let mut desired = Vec::with_capacity(layer.len());
            let mut weight = Vec::with_capacity(layer.len());
            for &i in layer {
                let mut nb: Vec<usize> = Vec::new();
                if mode != 1 {
                    nb.extend(lay.neighbors_up(i));
                }
                if mode != 0 {
                    nb.extend(lay.neighbors_down(i));
                }
                if nb.is_empty() {
                    desired.push(center[i]);
                    weight.push(0.3);
                } else {
                    let mut s = 0.0;
                    let mut ws = 0.0;
                    for &o in &nb {
                        let w = weights.get(&(i, o)).copied().unwrap_or(1.0);
                        s += w * center[o];
                        ws += w;
                    }
                    desired.push(s / ws);
                    weight.push(ws);
                }
            }
            let placed = place_layer(lay, widths, layer, &desired, &weight);
            for (&i, c) in layer.iter().zip(placed) {
                center[i] = c;
            }
        }
    }

    // Integer left edges, keeping gaps after rounding.
    let mut left = vec![0i32; total];
    for layer in &lay.layers {
        for (p, &i) in layer.iter().enumerate() {
            let mut x = (center[i] - widths[i] as f64 / 2.0).round() as i32;
            if p > 0 {
                let prev = layer[p - 1];
                let min_gap = (separation(lay, widths, prev, i) - (widths[prev] + widths[i]) as f64 / 2.0).round() as i32;
                x = x.max(left[prev] + widths[prev] + min_gap);
            }
            left[i] = x;
        }
    }

    // Layer heights and gaps; more room where many lines pass.
    let mut tops = vec![0i32; n_layers];
    let mut y = 0;
    for k in 0..n_layers {
        tops[k] = y;
        let h = lay.layers[k].iter().filter(|&&i| !lay.is_dummy(i)).map(|&i| heights[i]).max().unwrap_or(0);
        let lines_below = lay.links.iter().filter(|&&(u, _, _)| lay.layer_of[u] == k).count() as i32;
        y += h + LAYER_GAP + (lines_below / 3).min(10);
    }

    let min_x = (0..lay.real).map(|i| left[i]).min().unwrap_or(0);
    (0..lay.real).map(|i| (left[i] - min_x, tops[lay.layer_of[i]])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pav_pools_violators() {
        let y = pav(&[1.0, 3.0, 2.0, 4.0], &[1.0; 4]);
        assert_eq!(y, vec![1.0, 2.5, 2.5, 4.0]);
    }
}
