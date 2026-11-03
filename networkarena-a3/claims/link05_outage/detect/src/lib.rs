//! # outage_detect: did link 0-5 go down for a few seconds and come back?
//!
//! Reads `outage_switch`'s log (the logger is switch 0): 7-byte records `[tag][port][peer][at_ms u32 LE]`, tag 1 = a port went silent (at = its last keepalive),
//! tag 2 = heard again. Only the port whose peer is switch 5 counts (a port is that link if any record on it names switch 5).
//!
//! Rule: at least one silence of at least `MIN_DOWN_MS` that ended with the link coming back. A link that never fails, brief blips, a link that goes down and
//! stays down, and an outage on switch 0's other link (to switch 1) are false.

const MIN_OUTAGES: usize = 1;
const MIN_DOWN_MS: u32 = 2_000;
const PEER: u8 = 5;

fn detect(log: &[u8]) -> bool {
    let mut is_link = [false; 256];
    for r in log.chunks_exact(7) {
        if r[2] == PEER {
            is_link[r[1] as usize] = true;
        }
    }
    let mut down: [Option<u32>; 256] = [None; 256];
    let mut outages = 0;
    for r in log.chunks_exact(7) {
        let (tag, port) = (r[0], r[1] as usize);
        let at = u32::from_le_bytes([r[3], r[4], r[5], r[6]]);
        match tag {
            1 => down[port] = Some(at),
            2 => {
                if let Some(s) = down[port].take() {
                    if is_link[port] && at.saturating_sub(s) >= MIN_DOWN_MS {
                        outages += 1;
                    }
                }
            }
            _ => {}
        }
    }
    outages >= MIN_OUTAGES
}

switch_program_sdk::detect_program!(detect);

#[cfg(test)]
mod tests {
    use super::*;
    fn rec(tag: u8, port: u8, peer: u8, at: u32) -> Vec<u8> {
        let mut b = vec![tag, port, peer];
        b.extend_from_slice(&at.to_le_bytes());
        b
    }
    #[test]
    fn rules() {
        assert!(!detect(&[]));
        assert!(detect(&[rec(1, 1, 5, 30_000), rec(2, 1, 5, 36_000)].concat()), "six seconds down, back");
        assert!(!detect(&rec(1, 1, 5, 30_000)), "never came back");
        assert!(!detect(&[rec(1, 0, 1, 30_000), rec(2, 0, 1, 36_000)].concat()), "the other link");
        assert!(!detect(&[rec(1, 1, 5, 30_000), rec(2, 1, 5, 31_200)].concat()), "a blip");
        assert!(detect(&[rec(1, 1, 255, 30_000), rec(2, 1, 5, 36_000)].concat()), "peer learned late");
    }
}
