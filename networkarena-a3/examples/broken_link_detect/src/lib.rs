//! # broken_link_detect — the detector half of the solved practice egg
//!
//! The simulator calls `detect(log)` once, after the run, with the bytes
//! your logging switch wrote (`competitive_net_sim detect THIS.wasm
//! LOG`). It answers one question: is the egg in this run? Here the egg
//! is "link 1-2 goes down and stays down", and the log is `broken_link`'s:
//! `SILENT` and `HEARD` records, 9 bytes each, in the order the logging
//! switch got them.
//!
//! The rule: both ends of the link went silent and neither spoke again. A
//! port's last record decides, so a 5 s outage (silent, then heard) is
//! not the egg, and another link's silence is not this link's.
//!
//! Nothing here knows which run it is reading. Whatever the detector
//! needs, the switch program had to put in the log.

use std::collections::HashMap;

const TAG_SILENT: u8 = 1;
const TAG_HEARD: u8 = 2;
const LINK: (u8, u8) = (1, 2);

fn detect(log: &[u8]) -> bool {
    // (switch, neighbour) -> is that end silent now, by its last record.
    let mut silent: HashMap<(u8, u8), bool> = HashMap::new();
    for r in log.chunks_exact(9) {
        match r[0] {
            TAG_SILENT => silent.insert((r[1], r[2]), true),
            TAG_HEARD => silent.insert((r[1], r[2]), false),
            _ => None,
        };
    }
    let (a, b) = LINK;
    silent.get(&(a, b)) == Some(&true) && silent.get(&(b, a)) == Some(&true)
}

switch_program_sdk::detect_program!(detect);

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(tag: u8, switch: u8, neighbour: u8, at_ms: u32) -> Vec<u8> {
        let mut b = vec![tag, switch, neighbour, 0, 0];
        b.extend_from_slice(&at_ms.to_le_bytes());
        b
    }

    #[test]
    fn both_ends_silent_for_good_is_the_egg() {
        let down = [rec(TAG_SILENT, 1, 2, 16_000), rec(TAG_SILENT, 2, 1, 16_000)].concat();
        assert!(detect(&down));
        let back = [down.clone(), rec(TAG_HEARD, 1, 2, 20_000), rec(TAG_HEARD, 2, 1, 20_000)].concat();
        assert!(!detect(&back), "it came back");
        let other = [rec(TAG_SILENT, 3, 4, 16_000), rec(TAG_SILENT, 4, 3, 16_000)].concat();
        assert!(!detect(&other), "another link");
        assert!(!detect(&[]), "an empty log");
    }
}
