# Mode 01 PID 60 bitmap parsing fix

**Prepared:** 2026-10-03  
**Purpose:** Standalone explanation for the LogNook main thread.  
**Scope:** Documents the existing local change; no source files were edited while preparing this brief.

## What “PID 60” means here

`0x60` is the base of a Mode 01 *supported-PID bitmap range*. It is not itself a newly implemented live signal. A `0160` request asks which PIDs from `0x61` through `0x80` the ECU advertises. The reply contains the four-byte support bitmap and a continuation bit.

## Observed Scion reply

The live Scion transcript contains:

```text
TX 0160
RX 7E8 06 41 60 08 00 00 00
```

The attached regression fixture is `7E8 06 41 60 08 00 00 00`. The parser maps this bitmap to PID `0x65`, reports that no further bitmap range is advertised, and retains ECU ID `7E8`.

## Cause and code change

The supported-range parser already accepted range starts through `0xE0`, but the shared Mode 01 response-width table recognized only bitmap bases `0x00`, `0x20`, and `0x40` as four-byte responses. When a `0160` reply was parsed, the decoder treated `0x60` like an unknown signal PID and failed before reading its bitmap.

The local fix adds `0x60`, `0x80`, `0xA0`, `0xC0`, and `0xE0` to that table as four-byte bitmap replies. It does not add Mode 01 decoders for the PIDs those bitmaps may advertise.

Changed files already present in the working tree:

- `crates/lognook-core/src/obdlink_sx_protocol.rs`: recognize every later bitmap base through `0xE0` as a four-byte response.
- `crates/lognook-core/src/obdlink_sx_protocol_tests.rs`: add one fixture-backed Scion test and one table-driven test for all later bases.
- `crates/lognook-core/tests/fixtures/obdlink_sx/mode01_supported_60_header.txt`: preserve the raw ECU-headered Scion response.

## Regression coverage and limits

The fixture-backed test checks that range `0x60` yields only PID `0x65`, stops continuation, and records ECU `7E8`. The table-driven test checks `0x60`, `0x80`, `0xA0`, `0xC0`, and `0xE0` with a synthetic bitmap containing one PID and no continuation.

These tests were added in the existing local change. They were not run during preparation of this standalone brief, so this brief does not claim that the modified working tree passes them. The separate clean-worktree `cargo test --workspace` result recorded in the Scion validation report used the branch base and did not include these local changes.

The change affects Mode 01 capability discovery only. It does not add Mode 21/22 Toyota signals or guarantee that every advertised generic PID has an implemented decoder. PID `0x65` will become selectable only if it is also represented in LogNook’s standard signal catalog with the proper decoder.

Related live evidence: `results/scion-xb-2014-toyota-did-probe-2026-10-03.log` records the `0160` request and reply. The larger vehicle context is in `results/scion-xb-2014-live-app-validation-2026-10-03.md`.
