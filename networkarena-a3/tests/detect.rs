//! A detector built with the SDK's `detect_program!`, run on logs by
//! `competitive_net_sim::detect`.

#![cfg(feature = "wasm")]

use std::process::Command;
use std::sync::OnceLock;

use competitive_net_sim::detect::{run, Answer, DetectLimits};

/// The fixture detector (tests/fixtures/detector), built once.
fn fixture() -> &'static [u8] {
    static WASM: OnceLock<Vec<u8>> = OnceLock::new();
    WASM.get_or_init(|| {
        let root = env!("CARGO_MANIFEST_DIR");
        let manifest = format!("{root}/tests/fixtures/detector/Cargo.toml");
        let target_dir = format!("{root}/tests/fixtures/detector/target");
        let mut cmd = Command::new(env!("CARGO"));
        for (k, _) in std::env::vars() {
            if k.starts_with("CARGO_") {
                cmd.env_remove(k);
            }
        }
        let status = cmd
            .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
            .args(["--manifest-path", &manifest, "--target-dir", &target_dir])
            .status()
            .expect("cargo build of the detector fixture");
        assert!(status.success(), "the detector fixture did not build");
        let path = format!("{target_dir}/wasm32-unknown-unknown/release/detector_fixture.wasm");
        std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
    })
}

fn answer(log: &[u8]) -> Answer {
    run(fixture(), log, &DetectLimits::default())
}

#[test]
fn the_detector_answers_from_the_log() {
    assert_eq!(answer(b"T"), Answer::True);
    assert_eq!(answer(b"F"), Answer::False);
    assert_eq!(answer(b""), Answer::False, "an empty log is a log");
}

#[test]
fn a_log_far_past_a_switch_program_s_input_limit_arrives_whole() {
    let mut log = vec![b'x'; 64 * 1024];
    assert_eq!(answer(&log), Answer::False);
    *log.last_mut().unwrap() = b'!';
    assert_eq!(answer(&log), Answer::True, "the last byte of 64 KB was read");
}

#[test]
fn a_detector_that_never_answers_has_failed() {
    let failed = |a: Answer, why: &str| match a {
        Answer::Failed(w) => assert!(w.contains(why), "{w}"),
        other => panic!("expected a failure ({why}), got {other}"),
    };
    failed(answer(b"L"), "out of fuel");
    failed(answer(b"P"), "trap");
    failed(answer(b"G"), "memory limit");
}
