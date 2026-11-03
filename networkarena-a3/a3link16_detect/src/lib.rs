//! Detector for the a3link16 log: 5-byte records `[tag][at_ms u32 LE]`, tag 1 = link 1-6 silent
//! (at = last keepalive heard), tag 2 = heard again. The log holds nothing else.
//!
//! Egg rule (a bet; change the two constants to match what the claim card tells us):
//! link 1-6 went silent for at least `MIN_DOWN_MS`; if `PERMANENT`, it must also never come back.

const MIN_DOWN_MS: u32 = 1_000;
const PERMANENT: bool = false;
const RUN_END_MS: u32 = 40_000;

fn detect(log: &[u8]) -> bool {
    let mut down_since: Option<u32> = None;
    let mut longest = 0u32;
    for r in log.chunks_exact(5) {
        let at = u32::from_le_bytes([r[1], r[2], r[3], r[4]]);
        match r[0] {
            1 => down_since = Some(at),
            2 => {
                if let Some(s) = down_since.take() {
                    longest = longest.max(at.saturating_sub(s));
                    if PERMANENT {
                        return false; // it came back
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(s) = down_since {
        longest = longest.max(RUN_END_MS.saturating_sub(s));
    }
    if PERMANENT {
        down_since.is_some() && longest >= MIN_DOWN_MS
    } else {
        longest >= MIN_DOWN_MS
    }
}

switch_program_sdk::detect_program!(detect);

#[cfg(test)]
mod tests {
    use super::*;
    fn rec(tag: u8, at: u32) -> Vec<u8> {
        let mut b = vec![tag];
        b.extend_from_slice(&at.to_le_bytes());
        b
    }
    #[test]
    fn rules() {
        assert!(!detect(&[]));
        assert!(detect(&[rec(1, 5_000), rec(2, 7_500)].concat()));
        assert!(detect(&rec(1, 5_000)));
    }
}
