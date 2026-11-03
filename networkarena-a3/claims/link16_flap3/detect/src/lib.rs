//! # flap_detect: did link 1-6 go down and come back at least three times?
//!
//! Reads `flap_switch`'s log: 7-byte records `[tag][port][peer][at_ms u32 LE]`, tag 1 = a port went silent (at = its last keepalive),
//! tag 2 = heard again. Only the port whose peer is switch 1 (the logger is switch 6) counts.
//!
//! Rule: at least `MIN_FLAPS` silences of at least `MIN_DOWN_MS` that ended with the link coming back. A link that went down once, or went
//! down and stayed down, or a lossy link that never stays quiet for a second, is not a flap; neither is another switch's link.

const MIN_FLAPS: usize = 3;
const MIN_DOWN_MS: u32 = 1_000;
const PEER: u8 = 1;

fn detect(log: &[u8]) -> bool {
    // The logger may not have learned a port's peer when the port first went silent, so a port is link 1-6 if any record names switch 1.
    let mut is_link = [false; 256];
    for r in log.chunks_exact(7) {
        if r[2] == PEER {
            is_link[r[1] as usize] = true;
        }
    }
    let mut down: [Option<u32>; 256] = [None; 256];
    let mut flaps = 0;
    for r in log.chunks_exact(7) {
        let (tag, port) = (r[0], r[1] as usize);
        let at = u32::from_le_bytes([r[3], r[4], r[5], r[6]]);
        match tag {
            1 => down[port] = Some(at),
            2 => {
                if let Some(s) = down[port].take() {
                    if is_link[port] && at.saturating_sub(s) >= MIN_DOWN_MS {
                        flaps += 1;
                    }
                }
            }
            _ => {}
        }
    }
    flaps >= MIN_FLAPS
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
    fn log(v: &[Vec<u8>]) -> Vec<u8> {
        v.concat()
    }
    #[test]
    fn rules() {
        assert!(!detect(&[]));
        let three = log(&[rec(1, 0, 1, 5900), rec(2, 0, 1, 7600), rec(1, 0, 1, 17900), rec(2, 0, 1, 19700), rec(1, 0, 1, 29900), rec(2, 0, 1, 31600)]);
        assert!(detect(&three));
        assert!(!detect(&three[..28]), "two flaps are not enough");
        assert!(!detect(&three[..14]), "one flap is not enough");
        assert!(!detect(&three[..35]), "third never came back");
        let other = log(&[rec(1, 1, 5, 5900), rec(2, 1, 5, 7600), rec(1, 1, 5, 17900), rec(2, 1, 5, 19700), rec(1, 1, 5, 29900), rec(2, 1, 5, 31600)]);
        assert!(!detect(&other), "another link");
        let late = log(&[rec(1, 0, 255, 5900), rec(2, 0, 255, 7600), rec(1, 0, 1, 17900), rec(2, 0, 1, 19700), rec(1, 0, 1, 29900), rec(2, 0, 1, 31600)]);
        assert!(detect(&late), "peer learned after the first silence");
        let blips = log(&[rec(1, 0, 1, 5900), rec(2, 0, 1, 6300), rec(1, 0, 1, 17900), rec(2, 0, 1, 18400), rec(1, 0, 1, 29900), rec(2, 0, 1, 30300)]);
        assert!(!detect(&blips), "blips under a second");
    }
}
