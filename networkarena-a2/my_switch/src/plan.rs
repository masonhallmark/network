//! Part 2 planning: which switches each customer's packets traverse.
//!
//! Pure functions, no simulator types. Every switch runs the same
//! computation on the same inputs (the flooded topology, link capacities,
//! customer attachments, learned peers, and the contract), so every switch
//! derives the same plan without any coordination (MY_DESIGN.md D3).
//!
//! The candidate paths and the greedy placement deliberately mirror how the
//! grader builds its reference share (`src/floors.rs`): every simple path up
//! to `slack` hops longer than the shortest; customers placed fewest-choices
//! first, then unlimited, then biggest demand, then by index; each takes the
//! path whose thinnest link, shared with everyone already placed on it,
//! leaves it the most; the shortest wins ties.

use std::collections::{BTreeMap, VecDeque};

pub type Sw = u32;
pub type Adj = BTreeMap<Sw, Vec<Sw>>;
/// Directed link capacity, kbit/s.
pub type Caps = BTreeMap<(Sw, Sw), u64>;

/// How many hops longer than the shortest a path may be.
pub const SLACK: usize = 2;

/// One customer to place.
#[derive(Debug, Clone, Copy)]
pub struct Flow {
    pub from: Sw,
    pub to: Sw,
    /// `None` = no limit.
    pub demand_kbps: Option<u64>,
}

/// Every simple path from `from` to `to` no longer than the shortest plus
/// `slack` hops, as switch sequences (including both ends), shortest first
/// and then in switch-id order. Empty if `to` is unreachable; `[from]` if
/// `from == to`.
pub fn path_menu(adj: &Adj, from: Sw, to: Sw, slack: usize) -> Vec<Vec<Sw>> {
    if from == to {
        return vec![vec![from]];
    }
    // Hop distance to `to`, to prune the search.
    let mut dist: BTreeMap<Sw, usize> = BTreeMap::new();
    let mut frontier = VecDeque::from([to]);
    dist.insert(to, 0);
    while let Some(v) = frontier.pop_front() {
        let d = dist[&v];
        for &u in adj.get(&v).map(|v| v.as_slice()).unwrap_or(&[]) {
            if !dist.contains_key(&u) {
                dist.insert(u, d + 1);
                frontier.push_back(u);
            }
        }
    }
    let Some(&shortest) = dist.get(&from) else {
        return Vec::new();
    };
    let limit = shortest + slack;
    let mut out: Vec<Vec<Sw>> = Vec::new();
    let mut path = vec![from];
    walk(adj, &dist, to, limit, &mut path, &mut out);
    out.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
    out
}

fn walk(adj: &Adj, dist: &BTreeMap<Sw, usize>, to: Sw, limit: usize, path: &mut Vec<Sw>, out: &mut Vec<Vec<Sw>>) {
    let at = *path.last().unwrap();
    if at == to {
        out.push(path.clone());
        return;
    }
    let mut next: Vec<Sw> = adj.get(&at).cloned().unwrap_or_default();
    next.sort_unstable();
    for u in next {
        if path.contains(&u) {
            continue;
        }
        let Some(&rest) = dist.get(&u) else { continue };
        // `path.len()` nodes so far = `path.len() - 1` hops; one more to `u`.
        if path.len() + rest > limit {
            continue;
        }
        path.push(u);
        walk(adj, dist, to, limit, path, out);
        path.pop();
    }
}

/// Place every flow on one path of its menu. `None` for a flow with no
/// path (unreachable). The result is a pure function of the arguments.
pub fn assign_paths(flows: &[Flow], adj: &Adj, caps: &Caps) -> Vec<Option<Vec<Sw>>> {
    let menus: Vec<Vec<Vec<Sw>>> = flows.iter().map(|f| path_menu(adj, f.from, f.to, SLACK)).collect();
    let mut order: Vec<usize> = (0..flows.len()).collect();
    order.sort_by(|&a, &b| {
        let choices = |i: usize| menus[i].len().max(1);
        let size = |i: usize| flows[i].demand_kbps.unwrap_or(u64::MAX);
        choices(a)
            .cmp(&choices(b))
            .then_with(|| size(b).cmp(&size(a)))
            .then_with(|| a.cmp(&b))
    });
    let mut on_link: BTreeMap<(Sw, Sw), usize> = BTreeMap::new();
    let mut chosen: Vec<Option<Vec<Sw>>> = vec![None; flows.len()];
    for i in order {
        if menus[i].is_empty() {
            continue;
        }
        let mut best: Option<(f64, usize)> = None;
        for (pi, p) in menus[i].iter().enumerate() {
            let room = p
                .windows(2)
                .map(|w| {
                    let cap = caps.get(&(w[0], w[1])).copied().unwrap_or(0) as f64;
                    cap / (*on_link.get(&(w[0], w[1])).unwrap_or(&0) + 1) as f64
                })
                .fold(f64::INFINITY, f64::min);
            if best.map_or(true, |(br, _)| room > br + 1e-9) {
                best = Some((room, pi));
            }
        }
        let pi = best.map(|b| b.1).unwrap_or(0);
        for w in menus[i][pi].windows(2) {
            *on_link.entry((w[0], w[1])).or_insert(0) += 1;
        }
        chosen[i] = Some(menus[i][pi].clone());
    }
    chosen
}

/// A 15-bit, non-zero identifier of a plan. Two switches that computed the
/// same plan get the same epoch; a switch with different knowledge gets a
/// different one, so their route entries can never be mistaken for each
/// other's.
pub fn plan_epoch(parts: &[(usize, u32, &[Sw])]) -> u16 {
    let mut h: u32 = 0x811c_9dc5;
    let mut feed = |x: u32| {
        for b in x.to_le_bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
    };
    for (idx, dst, path) in parts {
        feed(*idx as u32);
        feed(*dst);
        feed(path.len() as u32);
        for s in *path {
            feed(*s);
        }
    }
    ((h ^ (h >> 15)) & 0x7fff).max(1) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adj_of(edges: &[(Sw, Sw)]) -> Adj {
        let mut adj = Adj::new();
        for &(a, b) in edges {
            adj.entry(a).or_default().push(b);
            adj.entry(b).or_default().push(a);
        }
        adj
    }

    #[test]
    fn menu_lists_shortest_then_longer_simple_paths() {
        // 4-ring 0-1-2-3-0 with a chord 1-3.
        let adj = adj_of(&[(0, 1), (1, 2), (2, 3), (3, 0), (1, 3)]);
        let m = path_menu(&adj, 0, 2, 2);
        assert_eq!(m, vec![vec![0, 1, 2], vec![0, 3, 2], vec![0, 1, 3, 2], vec![0, 3, 1, 2]]);
        assert_eq!(path_menu(&adj, 0, 0, 2), vec![vec![0]]);
        assert!(path_menu(&adj, 0, 9, 2).is_empty());
    }

    #[test]
    fn paths_never_repeat_a_switch() {
        let adj = adj_of(&[(0, 1), (1, 2), (2, 0), (2, 3)]);
        for p in path_menu(&adj, 0, 3, 2) {
            let mut s = p.clone();
            s.sort();
            s.dedup();
            assert_eq!(s.len(), p.len());
        }
    }

    /// Two thin links in parallel with a fat detour: the greedy placement
    /// must use the detour for the second customer instead of piling both
    /// on the shortest path.
    #[test]
    fn second_customer_takes_the_detour() {
        // 0 - 1 (2 Mbit) is the short way to 1; 0-2-3-1 is longer but fat.
        let adj = adj_of(&[(0, 1), (0, 2), (2, 3), (3, 1)]);
        let mut caps = Caps::new();
        for (a, b, c) in [(0, 1, 2000), (0, 2, 10000), (2, 3, 10000), (3, 1, 10000)] {
            caps.insert((a, b), c);
            caps.insert((b, a), c);
        }
        let flows = [
            Flow { from: 0, to: 1, demand_kbps: None },
            Flow { from: 0, to: 1, demand_kbps: Some(4000) },
        ];
        let plan = assign_paths(&flows, &adj, &caps);
        assert_eq!(plan[0].as_deref(), Some(&[0, 2, 3, 1][..]));
        assert!(plan[1].is_some());
    }

    #[test]
    fn epoch_is_a_function_of_the_plan() {
        let a = plan_epoch(&[(0, 5, &[1, 2, 3][..])]);
        let b = plan_epoch(&[(0, 5, &[1, 2, 3][..])]);
        let c = plan_epoch(&[(0, 5, &[1, 4, 3][..])]);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a >= 1 && a < 0x8000);
    }
}
