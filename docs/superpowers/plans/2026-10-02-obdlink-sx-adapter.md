# OBDLink SX Adapter Implementation Plan

> **Execution:** Inline implementation under `superpowers:executing-plans`. This uploaded workspace has no Git metadata, so commits and isolated Git worktrees are unavailable.

**Goal:** Add read-only OBDLink SX discovery and Mode 01 live acquisition through the existing `HardwareAdapter` contract.

> Scope note (2026-10-03): this completed plan defines the original Mode 01 adapter implementation only. It is not the project-wide physical-test policy; later field tests follow root `AGENTS.md`. The current production adapter still does not implement OEM/Mode 21 polling.

**Architecture (amended after physical validation):** The serial adapter opens the selected SX COM port after exact VID/PID verification, sends the adapter-only `STI` query to establish communication, validates the reply prompt, then queries `STDI` and `STSN`. An unsolicited startup banner is optional. Live acquisition discovers supported generic Mode 01 PIDs before sampling. Identity-only probing remains separate from live acquisition.

**Tech Stack:** Rust 2024, `serialport`, Tauri 2 commands, Svelte 5, Node UI tests.

**Spec:** `PROJECT_STATUS.md` next-task section; `LogNook_Blueprint/02_ARCHITECTURE.md`; official OBDLink Family Reference and Programming Manual Rev F (2025-08-29).

## Global Constraints

- Do not send commands to physical devices during implementation or tests; use fixtures and an injected fake serial stream.
- SX identity queries are `STI`, `STDI`, and `STSN` in that order; wait for the initial query's reply and for `>` between subsequent commands.
- For this adapter implementation, never send a bare carriage return, `ATZ`, `ATSP`, storage commands, clear-code commands, OEM/UDS requests, or actuator/write requests.
- This implementation's OBD traffic is limited to supported generic Mode 01 discovery (`0100`) and selected reads for its standard signals. Separate source-backed physical read probes follow root `AGENTS.md`; state-changing tests require per-test approval and are never part of normal acquisition.
- Match the SX by USB VID/PID `0403:6015`; do not match by FTDI vendor alone because OpenPort2 also has VID `0403`.
- Keep the existing `HardwareAdapter` contract, simulator, and external-agent reports intact.

## Review Focus

- Missing initial firmware reply/prompt: stop after the single read-only `STI` query without reset, bare CR, or OBD requests. A missing unsolicited startup banner alone is not an error.
- Wrong firmware stops after STI; wrong hardware identity stops after STDI. Assert no later serial/OBD queries.
- Unsupported PID: do not request it or emit a sample; test with a bitmap omitting that PID.
- Multiple ECUs or malformed frames: do not merge responders or fabricate values; test both cases.
- Unsupported physical device enumeration: OpenPort2 must not appear as an SX serial port; test exact VID/PID filtering.

---

### Task 1: Protocol fixtures and parsing

**Files:**
- Create: `crates/lognook-core/tests/fixtures/obdlink_sx/startup.txt`
- Create: `crates/lognook-core/tests/fixtures/obdlink_sx/identity.txt`
- Create: `crates/lognook-core/tests/fixtures/obdlink_sx/mode01_supported.txt` and per-signal response fixtures
- Create: `crates/lognook-core/src/obdlink_sx_protocol.rs`
- Modify: `crates/lognook-core/src/lib.rs`
- Test: unit tests in `obdlink_sx_protocol.rs`

**Interfaces:**
- Produces private helpers that parse Mode 01 service/PID/data bytes, parse the `0100` bitmap, identify multiple ECU response headers, and decode existing signal formulas.
- Fixtures are representative synthetic protocol fixtures based on documented framing, not claimed physical captures.

- [x] Add failing tests for the supported bitmap, PID decoding formulas, unsupported PIDs, malformed responses, and multiple ECU responders.
- [x] Run `cargo test -p lognook-core obdlink_sx_protocol::tests::supported_bitmap_selects_only_the_reported_generic_pids`; it failed at the unimplemented bitmap parser.
- [x] Implement the smallest parser helpers needed by those tests.
- [x] Re-run focused tests (5 passed) and `cargo test -p lognook-core` (12 passed). Temporary dead-code warnings remain until Task 2 uses the parser.

### Task 2: SX serial adapter and core engine path

**Files:**
- Create: `crates/lognook-core/src/obdlink_sx.rs`
- Modify: `crates/lognook-core/Cargo.toml`
- Modify: `crates/lognook-core/Cargo.lock`
- Modify: `crates/lognook-core/src/lib.rs`
- Modify: `crates/lognook-core/src/engine.rs`
- Test: unit tests in `obdlink_sx.rs`

**Interfaces:**
- Produces `ObdLinkSxAdapter::new(port_name: impl Into<String>)`, `identify_obdlink_sx(port_name) -> Result<DeviceDescriptor, LogNookError>`, `available_obdlink_sx_ports() -> Result<Vec<String>, LogNookError>`, and generic `LogNookEngine::connect_adapter(adapter, signal_ids)`.
- The adapter owns serial I/O and command sequencing; tests inject a fake `Read + Write` transport.

- [x] Add failing adapter tests for prompt-first behavior, bounded response size, exact identity/discovery command order, identity-only probing, refusal of non-SX identity, selected supported reads, unsupported signals, and exact VID/PID matching. Parser tests include multiple ECU replies on one or several lines and multiple-bitmap rejection.
- [x] Run focused tests and verify failures at the missing adapter, port matcher, and engine paths.
- [x] Add `serialport`, implement bounded prompt reads and the read-only adapter, and reuse the existing signal decoder and acquisition flow.
- [x] Filter enumerated USB serial ports to `VID=0x0403` and `PID=0x6015`; recheck the selected port before opening, and fail closed on multiple ECU responders or malformed/unsupported replies.
- [x] Run focused tests and full workspace tests.

### Task 3: Tauri and UI integration

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/App.svelte`
- Modify: `apps/desktop/src/app.css` only if needed for the existing toolbar layout
- Test: `apps/desktop/tests/workspace.test.mjs` only if new pure UI logic is added

**Interfaces:**
- Tauri commands: `available_obdlink_sx_ports() -> Vec<String>`, `identify_obdlink_sx(port_name)`, and `connect_obdlink_sx(port_name, signal_ids)`.
- The simulator remains available and uses the same engine path.

- [x] No new pure UI helper was needed; port and PID filtering stay in Rust.
- [x] Add an SX port picker, an adapter-only “Identify SX” action, and a separate “Connect OBDLink SX” action; show connected adapter identity and retain simulator controls.
- [x] Run Node UI tests (3 passed), `svelte-check` (0 errors/warnings), and Vite production build.

### Task 4: Project documentation and full verification

**Files:**
- Modify: `LogNook_Blueprint/02_ARCHITECTURE.md`
- Modify: `PROJECT_STATUS.md`
- Leave untouched: all files in `Work to do by Ext Agent/` and existing `results/` reports

- [x] Update architecture/status for serial discovery, strict read-only command scope, synthetic fixtures, hardware inventory, and the fact that no physical SX/vehicle validation has happened.
- [x] Run `cargo test --workspace` (26 passed), `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, the direct Node UI test command, `svelte-check`, and the Vite production build. npm is unavailable on PATH, so its local binaries were called directly.
- [x] Record environment installation and hardware-validation limits; no physical adapter compatibility is claimed.

Final review: self-review (subagent delegation disabled and this uploaded project has no Git metadata). Multiple-ECU framing and the response-size bound were fixed and covered before final verification; no remaining Critical or Important findings.

## Physical-validation follow-up — 2026-10-02

The user separately authorized physical connection tests after the original fixture-only implementation. Earlier checkboxes describe that initial implementation; the startup-banner assumption was superseded by the amendment above. The separate wake and ATSP experiments were one-off user-authorized probes and are not part of production acquisition.

- [x] Captured exact SX/Ford firmware, hardware, serial, and supported-PID responses at 115200 through D2XX and VCP.
- [x] Reproduced the no-banner bug with a failing regression using the physical captures.
- [x] Initiated the handshake with STI, tolerated optional startup frames, and required valid reply prompts before later commands.
- [x] Added regressions for physical identity-only probing, empty startup prompts, silent initial replies, and wrong firmware.
- [x] Verified the actual core HardwareAdapter on COM4: five advertised signals, one valid RPM read (632 rpm; 41 0C 09 E0), clean disconnect.
- [x] Ran all required checks: 31 Rust tests, Cargo check/Clippy/formatting; three UI tests, Svelte check, and Vite build passed.
- [x] Updated status, architecture, fixture provenance, and docs/hardware/2026-10-02-sx-ford.md with traces and test limits. External-agent reports were left intact.
