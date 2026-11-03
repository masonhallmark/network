//! A detector for tests/detect.rs. It answers from the log's first byte:
//! `T` true, `L` loop forever, `P` panic, `G` grab a gigabyte; anything
//! else, or an empty log, false. A log that ends in `!` is true whatever
//! it starts with, so a long log shows the whole of it arrived.

fn answer(log: &[u8]) -> bool {
    if log.last() == Some(&b'!') {
        return true;
    }
    match log.first() {
        Some(b'T') => true,
        Some(b'L') => loop {
            core::hint::black_box(());
        },
        Some(b'P') => panic!("asked to"),
        Some(b'G') => core::hint::black_box(vec![1u8; 1 << 30]).len() > 0,
        _ => false,
    }
}

switch_program_sdk::detect_program!(answer);
