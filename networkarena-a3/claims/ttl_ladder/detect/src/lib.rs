//! # ttl_detect: is a customer at the logging switch sending a TTL ladder?
//!
//! Reads `ttl_switch`'s log (tag 1: per second, the number of packets that entered from a customer port with TTL 0..15; tag 2: totals).
//! A ladder is a repeating run of small TTLs, so the rule asks for several different small values, each seen more than a few times, in
//! roughly even numbers, over several seconds. One constant low TTL (a flow that merely starts low), a handful of stray packets, or a
//! few seconds of one value do not count.
//!
//! The rule, over the whole log: among TTL 0..9, at least `MIN_DISTINCT` values with at least `MIN_EACH` packets each, the largest of them at
//! most `MAX_SPREAD` times the smallest, at least `MIN_PACKETS` such packets in all, and low-TTL packets in at least `MIN_SECONDS` different seconds.

const MIN_DISTINCT: usize = 4;
const MIN_EACH: u32 = 3;
const MAX_SPREAD: u32 = 4;
const MIN_PACKETS: u32 = 30;
const MIN_SECONDS: usize = 3;

fn detect(log: &[u8]) -> bool {
    let mut count = [0u32; 10];
    let mut seconds = std::collections::HashSet::new();
    let mut i = 0;
    while i < log.len() {
        match log[i] {
            1 if i + 34 <= log.len() => {
                let sec = log[i + 1];
                let mut any = false;
                for v in 0..10 {
                    let c = u16::from_le_bytes([log[i + 2 + 2 * v], log[i + 3 + 2 * v]]) as u32;
                    count[v] += c;
                    any |= c > 0;
                }
                if any {
                    seconds.insert(sec);
                }
                i += 34;
            }
            2 if i + 9 <= log.len() => i += 9,
            _ => break, // a short or unknown record: stop reading
        }
    }
    let seen: Vec<u32> = count.iter().copied().filter(|c| *c >= MIN_EACH).collect();
    let (min, max) = (seen.iter().copied().min().unwrap_or(0), seen.iter().copied().max().unwrap_or(0));
    seen.len() >= MIN_DISTINCT && max <= MAX_SPREAD * min && count.iter().sum::<u32>() >= MIN_PACKETS && seconds.len() >= MIN_SECONDS
}

switch_program_sdk::detect_program!(detect);

#[cfg(test)]
mod tests {
    use super::*;

    fn second(sec: u8, counts: &[(usize, u16)]) -> Vec<u8> {
        let mut v = [0u16; 16];
        for (i, c) in counts {
            v[*i] = *c;
        }
        let mut r = vec![1, sec];
        for c in v {
            r.extend_from_slice(&c.to_le_bytes());
        }
        r
    }

    #[test]
    fn a_ladder_is_found_and_look_alikes_are_not() {
        // a ladder 0..8, about 5 packets of each value a second, over 6 seconds
        let ladder: Vec<u8> = (3..9).flat_map(|s| second(s, &(0..9).map(|i| (i, 5)).collect::<Vec<_>>())).collect();
        assert!(detect(&ladder));
        // one constant low TTL
        let constant: Vec<u8> = (3..40).flat_map(|s| second(s, &[(4, 60)])).collect();
        assert!(!detect(&constant), "a constant low TTL is not a ladder");
        // a traceroute that sends only a few packets
        assert!(!detect(&second(9, &(0..9).map(|i| (i, 2)).collect::<Vec<_>>())), "a handful of packets in one second");
        // no low-TTL packets at all: only a total
        let mut none = vec![2];
        none.extend_from_slice(&0u32.to_le_bytes());
        none.extend_from_slice(&5000u32.to_le_bytes());
        assert!(!detect(&none));
        assert!(!detect(&[]));
        // one dominant value with a few others
        let skew: Vec<u8> = (3..9).flat_map(|s| second(s, &[(2, 100), (3, 4), (4, 4), (5, 4), (6, 4)])).collect();
        assert!(!detect(&skew), "one value dominating is not a ladder");
    }
}
