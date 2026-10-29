# Experiment tooling (A2 exploration)

Scratch tools used to answer the design-menu open questions. Not part of the submission.

- `analysis_bins/` — small Rust programs that read a `.simlog` (use as `src/bin/*.rs` of a crate that
  depends on `competitive_net_sim` by path): `loops` (packets revisiting a switch), `drops` (drops by
  kind/proto/reason/location), `hello` (max gap between hellos per port), `ctl` (control bytes per link),
  `rate` (a customer's send rate over time), `main` (packet census by kind/proto/size/src).
- `p3_param_switch/lib.rs` — A1 routing + per-customer queues configured at *build time* by the `CFG`
  env var, e.g. `CFG="mode=0;cap=4;misc=4;ctrl=4" cargo build --release --target wasm32-unknown-unknown`.
  Keys: `mode` (0 per-customer, 1 one shared data queue, 2 per-law), `cap`, `cap_{s,a,r,u}`, `drop`,
  `drop_{s,a,r,u}`, `w_{s,a,r,u}`, `ctrl`, `misc`, `w_ctrl`, `w_misc`, `disc` (0 DRR, 1 SP).
- `sweep.sh`, `p1.sh`, `b3.sh` — run all practice worlds / Part-1 worlds / build a config (paths are hardcoded
  to the session scratchpad; edit before reuse).
