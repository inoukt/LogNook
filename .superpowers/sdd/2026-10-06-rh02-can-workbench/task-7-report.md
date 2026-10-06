# Task 7 report — CAN Workbench analysis and export

## Implemented

- Saved capture reports now include deterministic per-ID census data and exact byte-value histograms, adapter/session provenance, paired JSONL errors, recorded route provenance, typed ISO-TP request/reply timing, raw PDUs, and NRC outcomes.
- Capture comparison reports IDs present on either side, frame-count deltas, and differing per-byte value distributions.
- Replay can accept bounded, timestamped common-pipeline samples; Rust `CanAnalyzer` produces labeled CAN-to-OBD Pearson correlation hypotheses only. The UI calls them hypotheses, not decoded or vehicle-validated signals.
- The existing serialized CAN owner supports manually marked, bounded pre/post trigger windows, records mark/completion/truncation metadata, emits the captured window, and exports it as paired raw CSV/JSONL with adapter provenance.
- DBC export lists observed CAN IDs and one placeholder signal per message. The file and each placeholder say `UNVERIFIED`; no inferred scale or signal meaning is asserted.
- Updated the hardware protocol architecture note and project status. RH-02 physical checks remain explicitly pending.

## Verification

- `cargo test --workspace`: **303 passed, 0 failed**.
- `cargo check --workspace`: passed; two existing `dead_code` warnings remain in `workers/lognook-j2534-x86/src/main.rs` (`read_bounded_line`, `discard_to_newline`).
- `npm run test:ui --prefix apps/desktop`: **101 passed, 0 failed**.
- `npm run build --prefix apps/desktop`: passed; Svelte check reported **0 errors and 0 warnings**, Vite build succeeded.
- Targeted `rustfmt --edition 2024 --config skip_children=true --check` on the changed Rust files: passed.
- `git diff --check`: passed.
- `cargo fmt --all -- --check`: ran and exited 1 because of pre-existing formatting diffs in untouched files, including `apps/desktop/src-tauri/src/commands/player.rs`, `commands/sessions.rs`, `crates/lognook-core/tests/derived_signals.rs`, and `workers/lognook-j2534-x86/src/registry.rs`. These unrelated files were left unchanged.

## Not verified / next task

No native Tauri window, save dialog, RH-02 USB adapter, or vehicle was available for a physical test. Windows device identity, listen-only support, bitrate, CAN RX/TX, and ECU replies remain unverified.

Next: connect the actual RH-02 on Windows to a known CAN bus, verify its selected interface and listen-only bitrate, capture and reopen raw frames, and confirm rates/flags and paired evidence. Then separately exercise the operator-started active scan/send path on a controlled bus and inspect the TX echo, raw replies, and timing report. Do not treat software fixtures as physical validation.

Commit: `9bdfb9f` (`feat: complete CAN workbench reports and trigger tools`).

## Independent-review follow-up

The follow-up commit addresses four verified findings:

- Disarming after marking now emits and preserves the incomplete trigger window. The sidecar records `trigger_ended_early` with `disarmed`, `capture_stopped`, or `session_ended`; disarm before marking cancels without creating a window. Full-window completion keeps its separate `trigger_completed` record.
- A zero-DLC CAN message remains in DBC output with an `UNVERIFIED` comment, but has no placeholder signal extending beyond its zero-byte payload.
- Diagnostic route labels and keys include 11-bit/29-bit width, so numerically identical standard and extended routes remain distinct.
- Correlation rows display OBD source and ECU labels, with explicit unknown-source/unknown-ECU fallbacks.

Follow-up verification: focused core analysis/workbench tests passed (**19 + 28**); focused Tauri report tests passed (**10**); UI tests passed (**103/103**); full `cargo test --workspace` passed (**309 passed, 0 failed**); `cargo check --workspace` passed with the same two existing J2534 worker dead-code warnings; `npm run build` passed with **0 Svelte errors/warnings**; changed-file rustfmt and `git diff --check` passed. Physical RH-02 verification remains pending.

Follow-up commit: `88ad6d9` (`fix: preserve partial CAN trigger windows and clarify reports`).

### Correlation-key follow-up

- CAN-to-OBD hypothesis rows now use a tuple key containing the CAN field, OBD signal, OBD source, and ECU. Hypotheses for the same signal/source from two ECUs therefore render as separate keyed rows.
- Added a UI regression with identical field/signal/source and ECU IDs `7E8` and `7E9`. It failed before the helper was added and passes with the ECU-aware key.
- `npm run test:ui --prefix apps/desktop`: **104 passed, 0 failed**.
- `npm run build --prefix apps/desktop`: passed; Svelte check reported **0 errors and 0 warnings**, and Vite built successfully.
- `git diff --check`: passed. The change contains no Rust files, so Rust formatting/tests were not rerun for this UI-only fix.
