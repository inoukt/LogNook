# Local Agent Control Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a same-PC AI agent inspect and operate LogNook through a PIN-unlocked MCP interface while all adapter I/O stays in the app’s existing owners.

**Architecture:** The installed LogNook executable also runs as an MCP stdio child when launched with `--agent-mcp`. It sends typed requests over an OS-local socket to the open, unlocked Tauri app. The app routes those requests to its existing acquisition, catalog, and CAN Workbench code; a separate bounded trace records raw adapter exchanges without changing the current metadata-only diagnostics.

**Tech Stack:** Rust 2024, Tauri 2, Svelte 5/TypeScript, official Rust MCP SDK `rmcp` 3.0.1, `interprocess` 2.4.4 local sockets, Argon2id PIN verifier, Tokio stdio runtime, serde/JSON, existing Rust and Node UI test suites.

**Spec:** [docs/superpowers/specs/2026-10-07-local-agent-control-design.md](../specs/2026-10-07-local-agent-control-design.md)

## Global Constraints

- Same PC only; MCP uses stdio and app IPC uses a same-user local endpoint. Do not open HTTP or LAN listeners.
- Agent Access starts locked; the UI owns PIN entry. Unlock expires after 15 minutes without an agent action; lock, timeout, or app exit revokes access and stops the agent’s active operation.
- After unlock, scoped tool calls do not require a second approval dialog. Keep a single local MCP host connected at a time.
- Allow one active agent operation per adapter and route all hardware requests through the existing serialized adapter owner.
- Connection or app startup must not start a scan or transmit a diagnostic request.
- Normal PID polling keeps its existing auto-request and exact-vehicle validation rules. Explicit Workbench scans may cover the full supported range only when requested, and must remain paced, cancellable, and recorded.
- Never put raw protocol bytes into `DiagnosticProgress`; its existing contract excludes raw payloads. Use a separate trace type and writer.
- Do not add automatic ECU programming, session/security changes, code clearing, actuator control, or firmware flashing. Manual CAN frames remain explicit Workbench actions and are recorded.
- Trace delivery and persistence must not block acquisition or drop recorded samples. Report trace gaps when a bounded live subscriber falls behind.
- Keep existing uncommitted OEM logging work out of implementation commits; execute implementation from a clean isolated checkout after preserving that work.

## Review Focus

- **Locked, expired, or closed app:** tools return a clear locked/unavailable result; expiration and manual lock cancel the active agent operation. Test in Tasks 1–2 and 8.
- **Malformed or oversized IPC/tool input:** reject invalid JSON, oversized messages, unsupported actions, invalid IDs, and out-of-range pacing before adapter I/O. Test in Tasks 2 and 8.
- **Competing UI and agent operations:** preserve one owner per adapter and do not apply live logging changes in states where the UI disallows them. Test in Tasks 3, 4, and 8.
- **Slow trace consumer or full queue:** trace gaps are counted and visible, while acquisition and recording continue. Test in Task 5.
- **Opaque or failed adapter responses:** retain success, NRC, timeout, malformed/wrong-route, disconnect, and unavailable-raw-data outcomes distinctly. Test in Tasks 6–7.

---

### Task 1: PIN Access and Settings UI

**Files:**
- Create: `apps/desktop/src-tauri/src/agent/access.rs`
- Create: `apps/desktop/src-tauri/src/agent/mod.rs`
- Create: `apps/desktop/src-tauri/src/commands/agent_access.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `apps/desktop/src/SettingsPanel.svelte`
- Modify: `apps/desktop/src/App.svelte`
- Test: `apps/desktop/src-tauri/src/agent/access.rs` unit tests

**Interfaces:**
- `agent/mod.rs` declares the access module and exposes its manager to Tauri commands and the runtime.
- Produces `AgentAccessStatus { configured: bool, unlocked: bool, expires_at_unix_ms: Option<u64> }` and typed `configure_agent_pin`, `unlock_agent_access`, `lock_agent_access`, and `agent_access_status` Tauri commands.
- PIN is six numeric digits. Persist only a salted Argon2id verifier; never persist or log the submitted PIN.
- Use `argon2` 0.6.0 with Argon2id and a salt from the OS random source; verify the stored encoded hash in constant time.
- Unlock refreshes a 15-minute idle lease. Agent operations call `authorize_action(now)`; UI lock and expired leases are observable by the runtime added in Task 2.

- [ ] **Step 1: Write access lifecycle tests** for `agent_access_starts_locked`, `wrong_pin_does_not_unlock`, `pin_verifier_does_not_contain_pin`, and `agent_access_expires_after_15_minutes_without_action`.
- [ ] **Step 2: Run the focused tests** with `cargo test -p lognook-desktop agent_access` and confirm the new tests fail before implementation.
- [ ] **Step 3: Implement `AgentAccessManager`** with PIN setup, salted verification, failed-attempt throttling, lease refresh, status, and manual lock; persist the verifier under the app data directory.
- [ ] **Step 4: Add the four typed Tauri commands and Settings controls** for set/change PIN, unlock, lock, and visible expiry/status. Keep the PIN input in the UI and never include it in an event or status result.
- [ ] **Step 5: Run `cargo test -p lognook-desktop agent_access` and `npm run build --prefix apps/desktop`;** confirm lifecycle tests pass and Svelte reports no errors.

### Task 2: Local IPC and MCP stdio Entry Point

**Files:**
- Modify: `apps/desktop/src-tauri/src/agent/mod.rs`
- Create: `apps/desktop/src-tauri/src/agent/ipc.rs`
- Create: `apps/desktop/src-tauri/src/agent/protocol.rs`
- Create: `apps/desktop/src-tauri/src/agent/mcp.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/main.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands/agent_access.rs`
- Test: `apps/desktop/src-tauri/src/agent/ipc.rs` and `mcp.rs` unit tests

**Interfaces:**
- `AgentRequest` is a closed serde-tagged enum; `AgentResponse` carries a typed success or a stable error code/message. Do not expose a generic Tauri command name or shell execution field.
- `run_agent_mcp() -> Result<(), String>` starts MCP stdio; `main` selects it only for the exact `--agent-mcp` argument, otherwise it starts Tauri normally.
- The app creates its local socket listener only while unlocked. Windows access is restricted with a current-user pipe security descriptor. The MCP child reports “open LogNook and unlock Agent Access” if no app endpoint is available.
- Use `rmcp = 3.0.1` with only server/macros/schema/stdio features; use `interprocess = 2.4.4` for local sockets and add only Tokio features required by stdio.

- [ ] **Step 1: Write IPC tests** for `ipc_rejects_oversized_message`, `ipc_rejects_unknown_action`, `ipc_requires_unlocked_session`, `ipc_reports_missing_app_endpoint`, and `ipc_allows_one_agent_host`.
- [ ] **Step 2: Add the minimal MCP test** `mcp_initializes_over_stdio_and_lists_status_tool`; confirm the tests fail before the server is implemented.
- [ ] **Step 3: Implement the current-user local socket listener and client** with bounded JSON messages and the `AgentRequest`/`AgentResponse` enum. Start/stop the listener with the access lease; locking or timeout must close it and invoke the active-operation cancel hook.
- [ ] **Step 4: Implement the `--agent-mcp` process branch** using `rmcp`; initially expose only `get_status`. Generate the host setup JSON from `current_exe()` plus `--agent-mcp`, and expose a **Copy agent setup** button in Settings.
- [ ] **Step 5: Run the focused Rust tests and `cargo check -p lognook-desktop`;** verify normal app launch still follows `run()`, the release Windows executable keeps redirected stdio when launched with `--agent-mcp`, and the MCP child writes protocol data only to stdout. If GUI-subsystem builds drop redirected handles, package a small console MCP binary and point the copied setup to it.

### Task 3: Adapter Status, Connection, and PID Selection

**Files:**
- Create: `apps/desktop/src-tauri/src/agent/tools.rs`
- Modify: `apps/desktop/src-tauri/src/agent/protocol.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `crates/lognook-core/src/engine.rs`
- Modify: `apps/desktop/src/App.svelte`
- Modify: `apps/desktop/src/types.ts`
- Modify: `apps/desktop/src/workspace.ts`
- Test: `apps/desktop/src-tauri/src/agent/tools.rs` and `apps/desktop/tests/agent-access.test.mjs`

**Interfaces:**
- `AgentAdapterChoice` is a typed union for simulator, OBDLink SX port, Bluetooth ELM327/A30 address, OpenPort provider, and Candlelight device/config.
- `list_adapters` reports available choices and supported capabilities; `connect_adapter(choice, log_signal_ids)` and `disconnect_adapter()` call existing constructors and the existing engine owner.
- `list_signals(query)` returns standard PID definitions. `set_signal_selection(log_signal_ids, graph_signal_ids)` applies graph changes immediately; log changes are rejected while connected/recording, matching the current UI behavior. The app persists accepted selections through its existing workspace-preference path.
- `get_status` returns the active adapter identity/capabilities, recording state, CAN Workbench state, current agent operation, and recent error summary.

- [ ] **Step 1: Write tests** `agent_status_reports_adapter_capabilities`, `agent_connect_reuses_existing_engine`, `agent_rejects_log_selection_change_while_connected`, and UI helper test `agent_signal_selection_updates_workspace_preferences`.
- [ ] **Step 2: Run `cargo test -p lognook-desktop agent_` and `npm run test:ui --prefix apps/desktop`;** confirm new tests fail before implementation.
- [ ] **Step 3: Add typed status/adapter/PID actions** to `AgentRequest` and dispatch them to existing adapter discovery/connect code and `LogNookEngine`; do not add another device owner.
- [ ] **Step 4: Add the `agent-signal-selection` UI event** and apply selections through the same state and preference helpers used by the PID list. Return a clear error when the current state blocks a logging change.
- [ ] **Step 5: Run the focused Rust/UI tests and desktop build;** confirm simulator and fixture adapters work without a physical adapter.

### Task 4: OEM Candidate and Mode 06 Tools

**Files:**
- Modify: `apps/desktop/src-tauri/src/agent/tools.rs`
- Modify: `apps/desktop/src-tauri/src/agent/protocol.rs`
- Reuse: `apps/desktop/src-tauri/src/commands/oem_catalog.rs`
- Reuse: `crates/lognook-core/src/oem_catalog.rs`
- Reuse: `crates/lognook-core/src/a30.rs`
- Test: `apps/desktop/src-tauri/src/agent/tools.rs`

**Interfaces:**
- `search_oem_candidates(query, brand, model, service, offset, limit)` returns source-backed candidate metadata and pagination.
- `try_oem_candidate(candidate_id)` runs one explicit existing candidate attempt and returns its typed outcome, raw response, route, timing, and evidence status.
- `list_mode06_tests()` returns the tests available for the active adapter. `try_mode06_test(test)` calls the existing A30 read-test path or returns an explicit unsupported-capability outcome.
- Neither action promotes catalog validation state or adds a candidate to automatic polling.

- [ ] **Step 1: Write tests** `agent_oem_search_preserves_source_and_route`, `agent_oem_try_returns_nrc_and_timeout_distinctly`, `agent_mode06_lists_adapter_capabilities`, and `agent_mode06_rejects_unsupported_adapter`.
- [ ] **Step 2: Run `cargo test -p lognook-desktop agent_`;** confirm the tests fail before adding the tool dispatch.
- [ ] **Step 3: Route tools through the existing `OemCatalog` and research execution helpers**; generate run metadata inside the agent service and return the existing typed results unchanged.
- [ ] **Step 4: Run the focused desktop tests plus `cargo test -p lognook-core oem_candidate_try`;** verify raw evidence remains separate from decoder/vehicle validation.

### Task 5: Communication Trace Store and Export

**Files:**
- Create: `crates/lognook-core/src/communication_trace.rs`
- Modify: `crates/lognook-core/src/lib.rs`
- Create: `apps/desktop/src-tauri/src/agent/trace.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/agent/protocol.rs`
- Modify: `apps/desktop/src-tauri/src/agent/tools.rs`
- Test: `crates/lognook-core/src/communication_trace.rs` and `apps/desktop/src-tauri/src/agent/trace.rs`

**Interfaces:**
- Add `CommunicationTraceEvent { sequence, timestamp_unix_ns, elapsed_ns, adapter_id, operation, direction, route, bytes_hex, outcome, raw_bytes_available }`. Keep it separate from `DiagnosticProgress`.
- Add nonblocking `CommunicationSink`; the sink uses `try_send` into a bounded queue. `get_trace(after_sequence, limit)` returns ordered events, the next cursor, coverage notes, and the number of omitted events; cap pages at 500 events.
- A user-started trace capture writes JSONL asynchronously under app data. The recent in-memory ring stays bounded; if a capture queue overflows or storage fails, record an explicit incomplete status and gap count rather than reporting a complete capture.

- [ ] **Step 1: Write tests** `trace_cursor_returns_ordered_events`, `trace_ring_reports_eviction`, `trace_slow_subscriber_reports_gap_without_blocking`, `trace_export_round_trips_raw_bytes`, and `trace_storage_error_is_reported`.
- [ ] **Step 2: Run `cargo test -p lognook-core communication_trace` and `cargo test -p lognook-desktop agent::trace`;** confirm new tests fail before implementation.
- [ ] **Step 3: Implement the trace event/sink and bounded store**; test sink insertion with a full queue and verify it returns promptly while incrementing the omission counter.
- [ ] **Step 4: Implement `get_trace`, start/stop trace capture, and JSONL export** in the agent service. Do not change diagnostics redaction or sample queues.
- [ ] **Step 5: Run trace tests and `cargo check --workspace`;** confirm the existing acquisition and recording tests still pass.

### Task 6: Instrument OBD Adapter Exchanges

**Files:**
- Modify: `crates/lognook-core/src/adapter.rs`
- Modify: `crates/lognook-core/src/obdlink_sx.rs`
- Modify: `crates/lognook-core/src/obdlink_sx/serial.rs`
- Modify: `crates/lognook-core/src/elm327.rs`
- Modify: `crates/lognook-core/src/a30.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/commands/bluetooth.rs`
- Modify: `apps/desktop/src-tauri/src/commands/oem_catalog.rs`
- Test: existing adapter unit tests plus focused additions in those modules

**Interfaces:**
- Add an optional `HardwareAdapter::set_communication_sink(CommunicationSink)` hook, parallel to the existing diagnostic sink.
- Instrument connect/identify and request/response boundaries for SX serial, Bluetooth ELM327 text exchanges, and A30 frames. Include TX/RX bytes and elapsed time when exposed; record an explicit unavailable reason instead of inventing bytes.
- Trace callbacks must be nonblocking and must not alter parsing, retry, or polling behavior.

- [ ] **Step 1: Add fixture assertions** `sx_trace_contains_exact_serial_exchange`, `elm_trace_contains_prompt_exchange`, and `a30_trace_contains_exact_frame_exchange`; confirm they fail before adding the sink.
- [ ] **Step 2: Thread the trace sink through the existing adapter creation and identify paths** without changing their transport ownership.
- [ ] **Step 3: Emit events at the lowest existing exchange boundary** in SX, ELM327, and A30; attach typed timeout/parse/response outcomes and avoid emitting secrets or PIN data.
- [ ] **Step 4: Run `cargo test -p lognook-core obdlink_sx`, `cargo test -p lognook-core elm327`, and `cargo test -p lognook-core a30`;** verify exact fixture bytes and unchanged adapter responses.

### Task 7: Instrument OpenPort2 and Candlelight Exchanges

**Files:**
- Modify: `crates/lognook-core/src/openport2.rs`
- Modify: `crates/lognook-core/src/openport2/worker_client.rs`
- Modify: `crates/lognook-core/src/candlelight.rs`
- Modify: `crates/lognook-core/src/can_workbench.rs`
- Test: existing OpenPort worker-client and CAN Workbench tests

**Interfaces:**
- Trace the exact serialized J2534 worker request/response and exposed CAN payloads at `worker_client.rs`; retain provider identity, route, timing, and typed worker errors. Do not claim visibility into vendor-driver USB traffic that J2534 does not expose.
- Feed Candlelight RX/TX frames to the same communication trace from the existing Workbench owner. Do not create a second USB reader; the existing capture remains the full-fidelity CAN evidence path.
- Surface adapter-specific trace coverage and missing-byte reasons through `get_trace`.

- [ ] **Step 1: Add tests** `openport_trace_records_worker_and_frame_outcomes`, `candlelight_trace_reuses_existing_owner`, and `trace_marks_driver_hidden_bytes_unavailable`; confirm they fail before implementation.
- [ ] **Step 2: Thread the sink through the existing J2534 client/worker and Candlelight owner** and emit transport events without changing the serialized request path.
- [ ] **Step 3: Verify CAN capture and trace describe the same frame sequence** when capture is enabled, including cancellation and provider disconnect outcomes.
- [ ] **Step 4: Run `cargo test -p lognook-core openport2` and `cargo test -p lognook-core can_workbench`;** confirm tracing does not change the J2534 worker protocol or CAN frame sequence.

### Task 8: CAN Workbench Tools

**Files:**
- Modify: `apps/desktop/src-tauri/src/agent/protocol.rs`
- Modify: `apps/desktop/src-tauri/src/agent/tools.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands/can.rs`
- Modify: `crates/lognook-core/src/engine.rs`
- Reuse: `crates/lognook-core/src/can_workbench.rs`
- Test: `apps/desktop/src-tauri/src/agent/tools.rs`

**Interfaces:**
- `workbench(action)` accepts only `start_capture`, `stop_capture`, `mark_event`, `start_scan`, `stop_scan`, and `send_frame` variants using existing `CanScanConfig`, `CanFrame`, route, pacing, and repeat validations.
- Agent captures use an app-data Workbench evidence path and never open a blocking file dialog. Return the saved capture/evidence ID or path.
- `send_frame` starts agent trace capture if needed so its raw TX/RX is retained. `stop_operation()` cancels the active agent scan/capture action and preserves partial evidence with a cancellation result. Listen-only CAN connections reject scan and send calls through the existing validation.

- [ ] **Step 1: Write tests** `agent_can_capture_uses_app_data_path`, `agent_workbench_rejects_invalid_range_before_tx`, `agent_rejects_second_operation_on_same_adapter`, `agent_send_frame_uses_configured_frame_once`, and `agent_lock_cancels_scan_with_partial_evidence`.
- [ ] **Step 2: Run `cargo test -p lognook-desktop agent_can` and `cargo test -p lognook-core can_workbench`;** confirm new agent tests fail before wiring the actions.
- [ ] **Step 3: Implement the closed action enum and dispatch** through `LogNookEngine`/`CanWorkbench`; retain exact scan config and raw TX/RX evidence. Connect remains non-transmitting.
- [ ] **Step 4: Run the focused tests** and verify invalid or oversized inputs cause no frame to reach the scripted transport.

### Task 9: User/Agent Guide and End-to-End Acceptance

**Files:**
- Create: `docs/AGENT_ACCESS.md`
- Modify: `apps/desktop/src/SettingsPanel.svelte`
- Modify: `apps/desktop/src-tauri/src/agent/mcp.rs`
- Modify: `PROJECT_STATUS.md`
- Test: `apps/desktop/tests/agent-access.test.mjs`

**Interfaces:**
- The guide gives the exact user setup steps, where to enter the PIN, how to stop/lock, how to export evidence, and a copyable task example.
- The guide states that the PIN is an app-use gate, not protection from other software already running as the same OS user.
- MCP server instructions begin with `get_status`, state how to interpret unsupported/negative/timeout results, and explain that raw evidence is not vehicle validation.
- Settings displays copied host JSON, agent connection/lock state, selected adapter, active operation, and expiry without exposing the PIN.

- [ ] **Step 1: Write UI acceptance tests** `agent_setup_copy_contains_current_executable`, `agent_locked_state_shows_next_step`, `agent_status_shows_trace_gap`, and `agent_user_guide_has_stop_and_export_steps`.
- [ ] **Step 2: Run `npm run test:ui --prefix apps/desktop`;** confirm the new tests fail before final UI/doc wiring.
- [ ] **Step 3: Complete the setup guide, MCP instructions, copyable host config, status UI, and project status note** using the approved spec’s user and agent wording.
- [ ] **Step 4: Run `cargo test -p lognook-core`, `cargo test -p lognook-desktop`, `cargo test -p lognook-j2534-protocol`, `npm run test:ui --prefix apps/desktop`, `npm run build --prefix apps/desktop`, and `cargo check --workspace`.**
- [ ] **Step 5: Perform a local MCP smoke test** with LogNook open: initialize the MCP client, confirm locked calls fail with the user instruction, unlock in Settings, read status and PID list, run one simulator test, inspect trace, lock, and confirm the next operation is cancelled/rejected. No physical vehicle is required for software acceptance.

## Implementation References

- The official Rust MCP SDK documents its server, schema, and stdio features: [rmcp README](https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/README.md).
- `interprocess` 2.4.4 provides portable local sockets, with a Windows named-pipe security descriptor hook: [local socket docs](https://docs.rs/interprocess/2.4.4/interprocess/local_socket/struct.ListenerOptions.html).
- `argon2` 0.6.0 provides Argon2id password hashing: [argon2 docs](https://docs.rs/argon2/0.6.0/argon2/).
