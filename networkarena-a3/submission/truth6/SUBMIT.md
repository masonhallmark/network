# Truth run candidate 6 = truth5 + congestion-control records (NOT uploaded)

File: `switch.wasm`, sha256 starts `ca41d71b22dcd90e` (198,724 bytes). Built from `a3truth6/` (a copy of `a3truth5/`; the frozen truth5 file is untouched).

## What is new
- EXT subtype 3 (CCA): per customer, every 5 s if it cut its rate in the window, and every 10 s regardless: cuts counted (a bin at most 70% of the one before that does not
  rebound), median post/pre %, climb slope, max, min, variation %, repeated dips, bin width (100 ms for 80+ packets/s, 500 ms for 16-80, nothing below).
- EXT subtype 4 (CCA_TRACE): 16 bins around a customer's first cut, three per customer at most (only with 100 ms bins).
- Lean mode (below 8 events/s or 45 B/s) makes none of them. Priority is below events and rates, one copy.
- `rec()` refuses any record whose body is not the length its type promises (a short body shifted every later record in the log; found by test).

## Before/after (same worlds, load 1.25; truth5 -> truth6)
See the table in the reply; raw lines in `tools/preflight/before_after.txt`.
