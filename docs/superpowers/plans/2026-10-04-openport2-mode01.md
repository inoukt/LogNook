# OpenPort 2.0 Mode 01 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Connect a Tactrix OpenPort 2.0 through a registered J2534 provider for read-only standard OBD-II Mode 01 testing in LogNook.

**Architecture:** A 32-bit Windows worker owns the selected registered J2534 04.04 DLL and communicates with the x64 app through bounded typed IPC. A core `HardwareAdapter` reuses shared Mode 01 decoding and sends samples through the existing engine pipeline; Tauri exposes provider selection and connects the adapter from the compact toolbar.

**Tech Stack:** Rust workspace, Windows J2534 FFI in an x86 worker, serde JSON-lines IPC, Tauri 2, Svelte 5, TypeScript, existing Node UI tests.

**Spec:** `docs/superpowers/specs/2026-10-04-openport2-mode01-design.md`

## Global Constraints

- Support only registered 32-bit J2534 API 04.04 providers named for Tactrix, OpenPort, or OPTA.
- Load the vendor DLL only in the isolated x86 worker; the worker is the single serialized owner of PassThru operations.
- Use standard 11-bit OBD-II Mode 01 over ISO 15765 at 500 kbit/s; no raw-CAN fallback.
- Permit only Mode 01 supported-PID discovery and selected catalog PID reads; expose no raw payload input or other service.
- The only J2534 transmissions are fixed service-01 read requests; never send ECU state-changing writes.
- Do not mix ECU responses; fail clearly if multiple responders appear.
- Reuse `HardwareAdapter`, shared measurement models, and the existing acquisition/recording/graph pipeline.
- Do not change drivers or perform vehicle state-changing writes, programming, session/security access, code clearing, routines, or actuator operations. J2534 `PassThruWriteMsgs` may carry only internally constructed Mode 01 read requests.

## Review Focus

- Missing or wrong-bitness provider DLL: verify discovery rejects invalid paths and reports the missing worker/provider clearly (Tasks 2–5).
- Malformed or oversized IPC and worker hang/exit: verify bounded parsing, timeout, acquisition stop, and cleanup (Tasks 2–4).
- Multiple ECU responders: verify they are rejected without merging samples (Task 4).
- Negative, truncated, wrong-service, wrong-PID, or wrong-ECU replies: verify they yield no valid sample (Tasks 1, 3, and 4).
- Failure after provider open or channel open: verify disconnect/close are attempted on every exit path (Task 3).

---

### Task 1: Share binary Mode 01 response parsing

**Files:**
- Modify: `crates/lognook-core/src/obdlink_sx_protocol.rs`
- Modify: `crates/lognook-core/src/lib.rs`
- Test: `crates/lognook-core/src/obdlink_sx_protocol_tests.rs`

**Interfaces:**
- Produce public `Mode01Response { ecu_id: Option<String>, raw_bytes: Vec<u8>, data: Vec<u8> }` and `SupportedPidRange { pids: Vec<u8>, has_more: bool, ecu_id: Option<String> }` types.
- Produce `parse_mode01_payload(ecu_id: String, payload: &[u8], requested_pid: u8) -> Result<Mode01Response, String>` for complete J2534 ISO 15765 payloads.
- Produce `parse_supported_pid_payload(ecu_id: String, payload: &[u8], range_start: u8) -> Result<SupportedPidRange, String>` and expose the existing signal decoder for adapter reuse.

- [x] **Step 1: Write failing parser tests** for an RPM response `41 0C 1A F8` (1726 rpm), a supported-PID bitmap, wrong service/PID, and truncated payload.
- [x] **Step 2: Run the focused test** with `cargo test -p lognook-core mode01_payload -- --nocapture`; confirm the new APIs/tests fail to compile or assert as expected.
- [x] **Step 3: Implement the typed payload parsers** by reusing existing PID widths and response validation; preserve the text parser used by SX.
- [x] **Step 4: Run `cargo test -p lognook-core obdlink_sx_protocol`** and confirm both existing SX parser tests and new binary-payload tests pass.
- [x] **Step 5: Commit** as `feat: expose typed Mode 01 payload parsing`.

### Task 2: Define the bounded worker protocol

**Files:**
- Create: `crates/lognook-j2534-protocol/Cargo.toml`
- Create: `crates/lognook-j2534-protocol/src/lib.rs`
- Create: `crates/lognook-j2534-protocol/tests/wire.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Produce serializable `J2534Provider { id: String, name: String }`.
- Produce a tagged `WorkerRequest` enum with only `ListProviders`, `Identify { provider_id }`, `Connect { provider_id }`, `DiscoverMode01`, `ReadMode01 { pid }`, and `Disconnect` variants.
- Produce `J2534Identity { firmware, dll_version, api_version }` and `Mode01Payload { ecu_id, bytes, latency_us }` structs.
- Produce a tagged `WorkerResponse` enum with `Providers { providers }`, `Identity { identity }`, `Connected { identity }`, `Mode01Responses { responses: Vec<Mode01Payload> }`, `Disconnected`, and `Error { kind, message }` variants.
- Define `WorkerErrorKind` values for missing provider/DLL, provider-open/API/channel failures, no response, timeout, malformed response, multiple responders, unsupported architecture, and IPC failure.
- Produce `encode_request_line`, `decode_request_line`, `encode_response_line`, and `decode_response_line`; each rejects serialized or received lines larger than 16 KiB and unknown variants.
- Define one JSON object per line; reject missing fields, malformed JSON, and oversized messages.

- [x] **Step 1: Create the minimal protocol package and failing round-trip tests** for each allowed message plus unknown variants, missing fields, and over-limit lines.
- [x] **Step 2: Run `cargo test -p lognook-j2534-protocol`;** confirm these tests fail before the message types are implemented.
- [x] **Step 3: Add the small serde-only protocol crate** and register it in the workspace; keep it independent of Tauri and hardware FFI.
- [x] **Step 4: Run `cargo test -p lognook-j2534-protocol`** and confirm all round-trip and rejection tests pass.
- [x] **Step 5: Commit** as `feat: define bounded J2534 worker protocol`.

### Task 3: Implement the x86 J2534 worker

**Files:**
- Create: `workers/lognook-j2534-x86/Cargo.toml`
- Create: `workers/lognook-j2534-x86/src/main.rs`
- Create: `workers/lognook-j2534-x86/src/registry.rs`
- Create: `workers/lognook-j2534-x86/src/j2534.rs`
- Create: `workers/lognook-j2534-x86/src/session.rs`
- Modify: `Cargo.toml`

**Interfaces:**
- Consume and emit the shared protocol crate's one-line messages on stdin/stdout; write diagnostics only to stderr.
- Enumerate the 32-bit `PassThruSupport.04.04` registry view, returning only provider entries whose registered names contain `Tactrix`, `OpenPort`, or `OPTA` and whose expanded DLL path exists.
- Resolve `provider_id` again from the registry before loading it; never accept a DLL path from the IPC caller.
- Implement provider identity through `PassThruOpen`, `PassThruReadVersion`, and `PassThruClose`; implement connection/read through ISO 15765 only.
- Compile a non-x86 test stub that exits with an explicit unsupported-architecture error; keep J2534 DLL loading behind the Windows x86 runtime check so pure worker tests can run on the current x64 host.
- Bind `PassThruWriteMsgs` only inside a helper that emits fixed service-01/pid reads; return every J2534 response message separately so responder IDs are never lost by concatenation.

- [x] **Step 1: Create the minimal worker crate and failing tests** for provider-name filtering, path expansion/existence, response matching, and cleanup after failures using a mocked PassThru function table.
- [x] **Step 2: Run `cargo test -p lognook-j2534-worker`;** confirm the new tests fail before the worker is implemented.
- [x] **Step 3: Implement the 32-bit-only worker** with bounded line handling, explicit J2534 04.04 exports, standard functional OBD Mode 01 requests, ISO 15765 response filtering, and disconnect/close guards. Call `PassThruWriteMsgs` only inside the fixed Mode 01 request helper.
- [x] **Step 4: Run `cargo test -p lognook-j2534-worker` and `cargo check -p lognook-j2534-worker --target i686-pc-windows-msvc`**; confirm pure tests pass and the Windows x86 code type-checks when the target libraries are installed. Record if the missing x86 linker prevents a full executable build.
- [x] **Step 5: Commit** as `feat: add isolated OpenPort J2534 worker`.

### Task 4: Add the core OpenPort adapter

**Files:**
- Create: `crates/lognook-core/src/openport2.rs`
- Create: `crates/lognook-core/src/openport2/worker_client.rs`
- Create: `crates/lognook-core/src/openport2/tests.rs`
- Modify: `crates/lognook-core/src/lib.rs`
- Modify: `crates/lognook-core/Cargo.toml`

**Interfaces:**
- Produce `available_openport2_providers(worker_path: &Path) -> Result<Vec<J2534Provider>, LogNookError>` and `identify_openport2(worker_path: &Path, provider_id: &str) -> Result<DeviceDescriptor, LogNookError>`.
- Produce `OpenPort2Adapter::new(worker_path: PathBuf, provider_id: String)` implementing `HardwareAdapter`.
- Use a private `WorkerSession` interface with `request(WorkerRequest) -> Result<WorkerResponse, LogNookError>` and `shutdown()` so adapter tests can inject a fake session.
- Keep one worker process for the adapter connection; use a response timeout and kill/report a worker that stops responding.
- Discover standard Mode 01 PID pages `00` through `E0`; read only selected supported signals, one PID at a time, at the existing 100 ms target interval.

- [x] **Step 1: Write failing adapter tests** with a fake worker session for provider identity, support discovery, timestamped RPM samples, mismatched ECU, multiple responders, malformed replies, timeout, and disconnect cleanup.
- [x] **Step 2: Run `cargo test -p lognook-core openport2`;** confirm the new tests fail before the adapter exists.
- [x] **Step 3: Implement the worker client and `HardwareAdapter`** using the typed binary Mode 01 parser from Task 1; reject unsupported signal IDs and never emit a sample after a malformed or ambiguous response.
- [x] **Step 4: Run `cargo test -p lognook-core openport2` and `cargo test -p lognook-core`**; confirm the fake-worker slice, shared-pipeline test, and existing engine tests pass.
- [x] **Step 5: Commit** as `feat: add OpenPort Mode 01 acquisition adapter`.

### Task 5: Expose OpenPort in Tauri and the compact toolbar

**Files:**
- Create: `apps/desktop/src-tauri/src/commands/openport2.rs`
- Create: `apps/desktop/src/connectionControls.ts`
- Test: `apps/desktop/tests/connection-controls.test.mjs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/App.svelte`
- Modify: `apps/desktop/src/types.ts`
- Modify: `apps/desktop/package.json`

**Interfaces:**
- Expose Tauri commands `available_openport2_providers`, `identify_openport2`, and `connect_openport2`.
- Resolve the worker from `target/i686-pc-windows-msvc/debug/lognook-j2534-worker.exe` for dev and the matching `release` directory for a release build; return an actionable error if it is not built.
- Add an adapter selector for Simulator, OBDLink SX, and OpenPort 2; show only the selected adapter's port/provider and identity controls, reusing one connect button so the toolbar stays on one row.
- Add pure connection-control helpers used by the toolbar and tests to verify selection/identity/connect enabled states.
- Keep worker packaging out of scope while Tauri bundling is disabled; the test build launches the worker from the repository's target directory.
- Add an npm script `build:j2534-worker` that builds package `lognook-j2534-worker` for `i686-pc-windows-msvc`; do not make simulator/SX launch depend on the helper.

- [x] **Step 1: Add failing UI helper tests** for adapter labels and disabled identify/connect states when no SX port or J2534 provider is selected.
- [x] **Step 2: Run `npm run test:ui`** and confirm the new helper tests fail.
- [x] **Step 3: Implement Tauri commands and compact adapter selection UI**; connect the returned `OpenPort2Adapter` through `LogNookEngine::connect_adapter` and retain existing SX/simulator behavior.
- [x] **Step 4: Run `npm run test:ui`, `npm run build`, and `cargo check -p lognook-desktop`**; confirm no toolbar wrapping or type errors are introduced.
- [x] **Step 5: Commit** as `feat: expose OpenPort testing controls`.

### Task 6: Document the test workflow and limitations

**Files:**
- Modify: `LogNook_Blueprint/02_ARCHITECTURE.md`
- Modify: `LogNook_Blueprint/03_HARDWARE_PROTOCOLS.md`
- Modify: `LogNook_Blueprint/README.md`
- Modify: `docs/hardware/2026-10-02-openport2.md`

- [x] **Step 1: Update architecture and project status** to distinguish implemented worker/Mode 01 support from pending physical validation; retain the earlier passive raw-CAN result as historical evidence.
- [x] **Step 2: Document exact build/launch steps** (`rustup target add i686-pc-windows-msvc`, `npm run build:j2534-worker`, then `npm run tauri dev`) and the required x86 MSVC linker/Windows SDK, plus provider selection and read-only request limits.
- [x] **Step 3: Run `git diff --check`** and review the docs against the implementation and spec.
- [x] **Step 4: Commit** as `docs: record OpenPort test workflow`.

## Final Verification

- Run `cargo fmt --all --check`.
- Run `cargo test -p lognook-j2534-protocol` and `cargo test -p lognook-core`.
- Build/check the worker for `i686-pc-windows-msvc` where the x86 target and linker exist.
- Run `npm run test:ui`, `npm run build`, and `cargo check -p lognook-desktop`.
- Do not claim physical vehicle compatibility until an OpenPort J2534 provider has returned standard PID responses through the app. Record the provider/API identity, ECU, latency, rate, and errors during that later test.
