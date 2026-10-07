# Vehicle Test Workbench Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Live-first test workbench where users can run quick adapter checks and sequentially test explicitly selected source-backed OEM candidates through the active acquisition adapter.

**Architecture:** Add a typed read-test request to `HardwareAdapter` and carry it to the adapter from the existing `AcquisitionHandle` worker, preserving one request owner. The Tauri commands revalidate candidate IDs against the read-only catalog; the Svelte workbench retains selections across search pages, runs one item at a time, shows results beside the existing graph, and exports a JSON report. Finite numeric results continue through the shared sample pipeline and active recording.

**Tech Stack:** Rust workspace, Tauri 2, Svelte 5/TypeScript, SQLite OEM catalog, serde/serde_json, existing acquisition and sample pipeline.

**Spec:** `docs/superpowers/specs/2026-10-05-vehicle-test-workbench-design.md`

## Global Constraints

- All vehicle requests go through the existing acquisition adapter owner. The workbench must not open a second serial/RFCOMM connection or start a competing worker while acquisition is connected.
- The acquisition owner gives due configured logging polls priority. It runs a queued research read between polling cycles when safe, then returns to logging. Research work may increase observed poll latency while a request is in flight; it must not discard samples already acquired.
- Catalog candidates are reloaded and validated on the backend before transmission. The request must retain the catalog's service, identifier width, ECU route, source revision, license, and decoder. No numeric identifier sweep, arbitrary raw command, state change, security access, session change, routine, or actuator action is available.
- Finite numeric decoded values publish as `Sample` events through the existing engine pipeline. Catalog values outside the catalog range remain graphable with `Suspect` quality. Unmapped Mode 06 numeric values remain `raw count`; DTC and VIN text do not create fabricated numeric signals.
- A recoverable ECU outcome advances to the next selected item. A lost adapter stops the run and leaves unstarted items available to run later. Stop is cooperative: the in-flight request completes or times out; no later item is sent. A user retry is a new recorded attempt. There is no automatic retry loop.
- OpenPort OEM queue actions remain unavailable until a read-only request path is implemented and tested for that adapter.
- Preserve the three existing untracked reports in `results/`; do not stage them.

## Review Focus

- A research read arrives while a live poll is due: verify the worker never runs both requests concurrently, serves due polls before later research requests, and resumes polling after the in-flight read.
- A typed ECU negative/timeout/malformed result versus adapter transport loss: continue on an ECU result; stop and mark later items skipped after link loss.
- A candidate ID is missing, changed, out of scope, or has a changed source revision: reject it before any adapter transmit.
- The selected candidate is on another search page when filters change: preserve the selected ID and its deterministic queue position.
- A decoder returns non-finite data or a DTC/VIN response contains numbers: retain raw evidence but publish no fabricated graph sample.

## File Map

- Create `crates/lognook-core/src/research.rs`: typed adapter research request/result enums, selected-item metadata, and JSON report types.
- Modify `crates/lognook-core/src/adapter.rs`: add the default-unsupported `HardwareAdapter::execute_research_test` contract.
- Modify `crates/lognook-core/src/acquisition.rs`: receive read-test commands on the existing worker channel and execute them on the owned adapter between read cycles.
- Modify `crates/lognook-core/src/engine.rs` and `lib.rs`: expose the connected-adapter research call and public types.
- Modify `crates/lognook-core/src/obdlink_sx/oem_probe.rs`, `elm327.rs`, and `a30.rs`: execute their supported research reads on the already-open session; preserve the one-shot protocol checks.
- Modify `apps/desktop/src-tauri/src/commands/oem_catalog.rs`, `commands/exports.rs`, `commands.rs`, and `lib.rs`: catalog-backed active-session requests, evidence run metadata, and a native JSON report export.
- Create `apps/desktop/src/researchQueue.ts` and `apps/desktop/tests/research-queue.test.mjs`: selected-ID retention, ordered execution, stop, and result progression.
- Modify `apps/desktop/src/OemCatalogPanel.svelte`, `App.svelte`, `types.ts`, and `app.css`; create `apps/desktop/src/ResearchRunResults.svelte` for the right-side result table.
- Update `LogNook_Blueprint/02_ARCHITECTURE.md`, `PROJECT_STATUS.md`, and the workbench design spec after verified behavior is complete.

## Tasks

### Task 1: Typed research-test contract

**Files:** Create `crates/lognook-core/src/research.rs`; modify `crates/lognook-core/src/adapter.rs` and `crates/lognook-core/src/lib.rs`.

**Interfaces:**
- `ResearchTestRequest::{OemCandidate(OemCandidateTrySpec), A30ReadTest(A30ReadTest)}`.
- `ResearchTestResult::{OemCandidate(OemCandidateTryResult), A30ReadTest(A30ReadTestResult)}`.
- `ResearchTestSelection::{OemCandidate(OemCandidate), A30ReadTest(A30ReadTest)}` is report metadata only; requests still use backend-reloaded candidate specs.
- `ResearchRunItem { queue_index: usize, queue_size: usize, selection: ResearchTestSelection, result: ResearchTestResult }`.
- `ResearchRunReport { run_id: String, adapter: DeviceDescriptor, started_at_unix_ms: u64, completed_at_unix_ms: u64, items: Vec<ResearchRunItem> }`.
- `HardwareAdapter::execute_research_test(&mut self, request: ResearchTestRequest) -> Result<ResearchTestResult, LogNookError>` defaults to a clear unsupported-read error.

- [ ] **Step 1: Add contract tests** `unsupported_adapter_rejects_research_test` and `research_result_round_trips_with_tagged_variant`; assert the default adapter performs no request and the result keeps its variant.
- [ ] **Step 2: Run the focused core tests** with `cargo test -p lognook-core research::`; confirm the missing types/method fail compilation.
- [ ] **Step 3: Implement the enums, serde tags, and default trait method** with no transport-specific policy in the shared contract.
- [ ] **Step 4: Run `cargo test -p lognook-core research::`** and confirm both tests pass.
- [ ] **Step 5: Commit** as `feat: define typed adapter research requests`.

### Task 2: Run supported reads on each connected adapter

**Files:** Modify `crates/lognook-core/src/obdlink_sx/oem_probe.rs`, `elm327.rs`, and `a30.rs`, with focused tests beside those modules.

**Interfaces:** Implement `HardwareAdapter::execute_research_test` for SX, ELM327, and A30. Each method uses the already-open adapter/session and returns the typed result from Task 1. Simulator and OpenPort retain the default unsupported result.

- [ ] **Step 1: Add tests** `sx_candidate_reuses_connected_session`, `elm_candidate_reuses_paired_stream`, `a30_candidate_and_captured_read_reuse_open_session`, and `unsupported_adapter_does_not_transmit`; assert no second connect/open occurs and each adapter emits only the exact catalog/captured read request.
- [ ] **Step 2: Run the focused adapter tests**; confirm the methods are missing or currently reopen independent sessions.
- [ ] **Step 3: Refactor the existing SX/ELM/A30 one-shot helpers** to call their connected-session request methods; retain catalog route/echo validation and A30's captured checksum/session framing.
- [ ] **Step 4: Run all three adapter test modules** and confirm existing one-shot, Mode 01, Mode 21/22, and A30 Mode 02/03/06/07/09/0A tests pass.
- [ ] **Step 5: Commit** as `refactor: run research reads on connected adapters`.

### Task 3: Serialize research requests through acquisition

**Files:** Modify `crates/lognook-core/src/acquisition.rs`, `engine.rs`, and their existing unit tests.

**Interfaces:** `AcquisitionHandle::execute_research_test(request: ResearchTestRequest) -> Result<ResearchTestResult, LogNookError>` sends one request to the acquisition worker and waits for its typed result. `LogNookEngine::execute_research_test(request: ResearchTestRequest) -> Result<ResearchTestResult, LogNookError>` requires an active adapter and delegates to that handle.

- [ ] **Step 1: Add fake-adapter tests** `research_request_runs_on_acquisition_owner`, `research_request_waits_for_live_poll`, and `research_request_error_returns_without_overlapping_poll`; assert max concurrent adapter calls is one, due polls precede later research requests, and samples before/after the research call are retained.
- [ ] **Step 2: Run the focused acquisition tests**; confirm the current stop-only channel cannot route a research request.
- [ ] **Step 3: Add a typed worker command/reply channel**. Process each request on the thread that owns `HardwareAdapter`; service a due logging poll before accepting another research request. Return a request error without corrupting the command channel; the next normal poll detects actual transport loss.
- [ ] **Step 4: Add the engine method** with a clear not-connected error and preserve the worker's existing fatal/transient acquisition behavior.
- [ ] **Step 5: Run `cargo test -p lognook-core acquisition::` and the engine pipeline tests**; verify serialized order and retained samples.
- [ ] **Step 6: Commit** as `feat: serialize research reads with acquisition`.

### Task 4: Tauri commands, evidence, and report export

**Files:** Modify `apps/desktop/src-tauri/src/commands/oem_catalog.rs`, `commands/exports.rs`, `commands.rs`, and `lib.rs`; update focused Tauri command tests.

**Interfaces:**
- Candidate Try Once and A30 read-test commands use `LogNookEngine::execute_research_test`; they no longer accept a port/Bluetooth address or open a second adapter.
- Candidate command input is a `candidate_id`; the backend calls `OemCatalog::candidate_for_try` immediately before sending.
- Each item carries `run_id`, one-based `queue_index`, and `queue_size` into its existing evidence JSONL envelope.
- `export_research_run_report(app: AppHandle, report: ResearchRunReport) -> Result<Option<String>, String>` opens a JSON save dialog and refuses to overwrite a file. `ResearchRunReport` is the serializable type defined in Task 1.

- [ ] **Step 1: Add tests** `candidate_is_reloaded_before_active_session_request`, `research_evidence_records_run_order`, and `run_report_json_round_trips_and_never_overwrites`; assert an invalid/stale candidate produces no adapter request.
- [ ] **Step 2: Run the focused desktop command tests**; verify the current commands reject connected/recording use and open separate adapter sessions.
- [ ] **Step 3: Route candidate and A30 requests through the active engine**. Keep successful numeric publication on `publish_research_sample`; append typed outcomes/raw evidence with the run metadata even when the ECU does not return a positive response.
- [ ] **Step 4: Add typed `ResearchRunReport` serialization and native JSON export** using the existing dialog plugin and create-new file semantics.
- [ ] **Step 5: Run Tauri command tests and `cargo test -p lognook-desktop`**; confirm evidence and export behavior.
- [ ] **Step 6: Commit** as `feat: route workbench tests through active adapter`.

### Task 5: Selection and sequential queue state

**Files:** Create `apps/desktop/src/researchQueue.ts` and `apps/desktop/tests/research-queue.test.mjs`.

**Interfaces:**
- `toggleCandidateSelection(current: readonly string[], candidateId: string, selected: boolean): string[]` retains selection order and removes only the unchecked ID.
- `runResearchQueue<T>(items: readonly string[], runOne: (id: string, index: number) => Promise<T>, shouldStop: () => boolean, onItem: (id: string, index: number, result: T | Error) => void): Promise<void>` awaits each request before starting the next; a rejected request is reported and stops the queue, while a typed ECU outcome is a normal result.

- [ ] **Step 1: Add tests** `selection_survives_search_and_page_changes`, `queue_awaits_each_item_in_selection_order`, `stop_keeps_current_result_and_skips_later_items`, and `transport_error_stops_before_later_items`.
- [ ] **Step 2: Run `npm run test:ui`**; confirm the new helper imports fail.
- [ ] **Step 3: Implement the pure selection and sequential-run helpers** without adding timers, background workers, or auto-retries.
- [ ] **Step 4: Run the focused Node tests and `npm run test:ui`**; verify typed negative/no-response results allow the next item while a rejected transport call stops it.
- [ ] **Step 5: Commit** as `feat: add sequential OEM research queue`.

### Task 6: Live-first workbench UI and report flow

**Files:** Modify `apps/desktop/src/OemCatalogPanel.svelte`, `App.svelte`, `types.ts`, and `app.css`; create `apps/desktop/src/ResearchRunResults.svelte` and `apps/desktop/tests/research-workbench.test.mjs`.

**Interfaces:** Replace the **OEM Research** view label with **Test Workbench** while preserving the **Live PIDs** view. Keep adapter controls and the current graph in place. The workbench exposes adapter-specific quick tests, candidate checkboxes, selected count, **Run selected** / **Stop**, per-item result rows below the graph, and **Export run report**.

- [ ] **Step 1: Add UI tests** `workbench_shows_only_supported_quick_tests`, `candidate_selection_survives_catalog_navigation`, `run_selected_reports_progress_in_order`, `stop_does_not_send_the_next_request`, `numeric_result_updates_graph_while_recording`, and `export_uses_completed_run_report`.
- [ ] **Step 2: Run `npm run test:ui`**; confirm these workbench behaviors are not implemented.
- [ ] **Step 3: Add persistent selected-ID state and checkboxes** to the paged OEM catalog; retain the existing per-candidate **Try once** action and route it through the connected adapter owner.
- [ ] **Step 4: Wire the sequential helper** to the candidate/A30 Tauri commands; show current item, counts, typed outcome, latency, ECU, and Stop behavior. Stop only prevents the next request.
- [ ] **Step 5: Add the right-side results table below the existing graph** and send finite numeric results through existing graph callbacks; preserve live PID polling and recording controls while the workbench is open.
- [ ] **Step 6: Add Export run report** and adapter-specific quick-test controls without raw-hex input or new graph state.
- [ ] **Step 7: Run focused UI tests, `npm run test:ui`, and `npm run build`**; require Svelte 0 errors/0 warnings and a successful Vite build.
- [ ] **Step 8: Commit** as `feat: add live-first vehicle test workbench`.

### Task 7: Documentation and merged verification

**Files:** Update `LogNook_Blueprint/02_ARCHITECTURE.md`, `PROJECT_STATUS.md`, and the design spec with implemented behavior and test results.

- [ ] Document active-adapter request ownership, supported quick tests by adapter, queue outcomes/stop behavior, report export, graph/recording flow, and the physical validation boundary.
- [ ] Run `cargo test --workspace -j 1`, `cargo check --workspace -j 1`, `cargo build -p lognook-desktop --features tauri/custom-protocol -j 1`, `npm run test:ui`, `npm run build`, targeted `rustfmt --check`, and `git diff --check`.
- [ ] Review the full diff and preserve the three untracked audit reports.
- [ ] Record that the final hardware check still requires an actual connected adapter; do not claim A30 physical validation from fixture tests.
- [ ] Commit documentation after the user reviews the completed work.
