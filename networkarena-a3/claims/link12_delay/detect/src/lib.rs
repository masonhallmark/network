//! # delay_detect: does link 1-2 have a slow direction (switch 2 -> switch 1) and a fast one?
//!
//! Reads `delay_switch`'s log: 7-byte records `[tag][second][count][min u16][mean u16]`, delays in 100 us. Tag 1 = delay of 2 -> 1, tag 2 = of 1 -> 2.
//! A window's `min` is the propagation delay (queueing only adds to it), so the rule uses the medians of the window minimums:
//! - 2 -> 1 is slow: median min at least `SLOW` (10 ms), steady (at least 70% of windows within `STEADY` of the median, and mean-min small), over at least `MIN_WINDOWS` windows;
//! - 1 -> 2 is at most 1/`RATIO` of it, when we have its windows (if its reports never arrived, the slow side alone decides).
//! A fast link, a symmetric slow link, a slow link the other way round, or a link that is only congested (delay varies) are false.

const SLOW: u32 = 100; // 10 ms
const STEADY: u32 = 30; // 3 ms
const RATIO: u32 = 4;
const MIN_WINDOWS: usize = 5;

fn median(v: &mut Vec<u32>) -> u32 {
    v.sort_unstable();
    v[v.len() / 2]
}

fn parse(log: &[u8], tag: u8) -> Vec<(u32, u32)> {
    log.chunks_exact(7)
        .filter(|r| r[0] == tag && r[2] > 0)
        .map(|r| (u16::from_le_bytes([r[3], r[4]]) as u32, u16::from_le_bytes([r[5], r[6]]) as u32))
        .collect()
}

fn detect(log: &[u8]) -> bool {
    let slow = parse(log, 1);
    if slow.len() < MIN_WINDOWS {
        return false;
    }
    let mut mins: Vec<u32> = slow.iter().map(|w| w.0).collect();
    let m = median(&mut mins);
    let steady = slow.iter().filter(|w| w.0.abs_diff(m) <= STEADY && w.1.saturating_sub(w.0) <= STEADY).count();
    if m < SLOW || steady * 10 < slow.len() * 7 {
        return false;
    }
    let fast = parse(log, 2);
    if fast.len() >= MIN_WINDOWS {
        let mut f: Vec<u32> = fast.iter().map(|w| w.0).collect();
        return median(&mut f) * RATIO <= m;
    }
    true
}

switch_program_sdk::detect_program!(detect);

#[cfg(test)]
mod tests {
    use super::*;
    fn log(tag: u8, mins: &[u16]) -> Vec<u8> {
        let mut b = Vec::new();
        for (i, m) in mins.iter().enumerate() {
            b.extend_from_slice(&[tag, i as u8, 9]);
            b.extend_from_slice(&m.to_le_bytes());
            b.extend_from_slice(&(m + 2).to_le_bytes());
        }
        b
    }
    #[test]
    fn rules() {
        assert!(!detect(&[]));
        let slow = log(1, &[300; 20]);
        assert!(detect(&[slow.clone(), log(2, &[20; 20])].concat()), "30 ms one way, 2 ms the other");
        assert!(detect(&slow), "reports never arrived");
        assert!(!detect(&[slow.clone(), log(2, &[300; 20])].concat()), "slow both ways");
        assert!(!detect(&[log(1, &[5; 20]), log(2, &[300; 20])].concat()), "slow the other way round");
        assert!(!detect(&[log(1, &[5; 20]), log(2, &[5; 20])].concat()), "normal link");
        let jumpy: Vec<u16> = (0..20).map(|i| if i % 2 == 0 { 120 } else { 900 }).collect();
        assert!(!detect(&log(1, &jumpy)), "congested, not a fixed delay");
        assert!(!detect(&log(1, &[300; 3])), "too few windows");
    }
}
