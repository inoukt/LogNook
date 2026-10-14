# Saved CAN Capture Replay Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Prepare one exact CAN ID/time segment from a saved LogNook capture and explicitly transmit it once through the existing CAN Workbench, with equivalent UI and MCP access.

**Architecture:** Stream the capture into a bounded immutable backend preview. Start resolves a token to a validated replay plan and submits it to the current serialized CAN owner. The source file stays unchanged and a different paired capture stores TX evidence.

**Tech Stack:** Existing Rust/Tauri/Svelte/TypeScript, CSV and serde; add sha2 for source SHA-256 if no existing direct SHA-256 dependency is available.

**Spec:** [Proposed design](../specs/2026-10-13-saved-can-replay-vehicle-profiles-design.md), saved replay sections.

## Global constraints

- Input: LogNook `.candump.csv`, maximum 256 MiB; one exact ID/format; RX, flags zero, classic payload.
- Saved clip: 1–10,000 frames; source and effective paced duration each at most 600 seconds. Live clip remains 1–400.
- Original gaps at most 60 seconds; replay floor 1 ms; original order; one pass; no catch-up.
- Preview token expires after 10 minutes; replacement invalidates old token. No silent truncation.
- Active target, different open paired output capture, existing serialized owner and unlocked Agent Access for MCP.
- No adapter, driver, firmware or catalog changes. Source timestamps and raw payloads stay unchanged.

## Review focus

- A malformed row outside the selected range rejects preparation before any TX.
- A replaced/changed file after preview cannot alter the prepared sequence.
- Standard/extended IDs with the same integer stay distinct; TX echoes are never replay sources.
- Disconnect/reconnect and capture replacement invalidate an old target/Start request.
- Storage failure after TX preserves the submitted count and cancels remaining work.

## Task 1: File-to-plan preparation

**Files:** Create `crates/lognook-core/src/can_replay.rs`, `crates/lognook-core/tests/saved_can_replay.rs`; modify core `Cargo.toml` and `src/lib.rs`.

**Interfaces:** `SavedCanReplaySelection { can_id: u32, extended: bool, start_elapsed_ns: i64, end_elapsed_ns: i64 }`; `CanReplaySource` distinguishes LiveSnapshot from SavedCapture provenance (digest, original rows/range, optional bitrate). `CanReplayPlan` contains private validated frames/source plus getters. `prepare_saved_can_replay(path: &Path, selection: SavedCanReplaySelection, cancelled: &AtomicBool) -> Result<PreparedSavedCanReplay, LogNookError>` produces a plan and `SavedCanReplaySummary` with source/exclusion counts, first/last rows, span and paced duration. Use `CanCaptureReader` rather than a second parser.

- [ ] Write fixtures/assertions for mixed IDs/formats, RX/TX/flags exclusions, inclusive boundaries, retained original row/timestamps, duplicate timestamps, malformed rows outside range, decreasing matching timestamps, empty input, checked overflow, 10,000 versus 10,001 selected frames, both duration bounds, oversized file and cancellation. Assert explicit errors and zero fake-transport submissions.
- [ ] Run `cargo test -p lognook-core --test saved_can_replay`; implement streaming selection, SHA-256 and checked validation until fixtures pass. Metadata/sidecar inspection remains at the desktop boundary.
- [ ] Provide a LiveSnapshot constructor retaining the current 400-frame limits and validate the plan again before handing it to a worker.

## Task 2: Shared worker and source evidence

**Files:** Modify `crates/lognook-core/src/can_workbench.rs`, `src/can_workbench/worker.rs`, `src/engine.rs`; extend `tests/can_workbench.rs` and worker evidence tests.

**Interfaces:** `CanWorkbench::start_prepared_can_replay(plan: CanReplayPlan) -> Result<CanIdReplayStatus, LogNookError>` and engine forwarding. Keep `start_can_id_replay(Vec<CanFrame>)` as the live compatibility wrapper. Add engine CAN connection generation query for guarded desktop Start. Extend source evidence with saved provenance and chunks at most 400 frames.

- [ ] Pin live 400-frame behavior and saved >400 behavior independently; assert exact order, minimum pacing, no catch-up, manual-send/scan mutual exclusion, one pass, and Stop/capture close/disconnect cancellation.
- [ ] Implement plan transport through the current worker. Journal and flush provenance/source chunks before TX. Require known source bitrate to equal the active target; preserve actual target identity/channel in evidence.
- [ ] Test failed initial evidence writes produce zero TX; failed subsequent writes retain submitted count; replay after reconnect cannot enter the new worker; old live evidence fixtures remain readable.
- [ ] Run `cargo test -p lognook-core --test saved_can_replay --test can_workbench` and relevant core library worker tests.

## Task 3: Backend preview service and typed commands

**Files:** Create `apps/desktop/src-tauri/src/can_replay_previews.rs`, `src/commands/saved_can_replay.rs`; register in `src/commands.rs` and `src/lib.rs`; reuse `commands/can.rs` sidecar handling and `agent/log_data.rs` source resolution.

**Interfaces:** one preview registry holding a plan, opaque ID, expiry and cancellation generation. Tauri commands: `prepare_saved_can_replay`, `get_saved_can_replay_preview`, `discard_saved_can_replay_preview`, `start_saved_can_replay(preview_id, expected_can_generation)`. Start rechecks target/capture/generation and takes the frozen plan atomically. UI native file selection can register a session-local saved source for MCP.

- [ ] Test stale/expired/replaced IDs, currently written input, path aliasing, missing sidecar, bitrate mismatch, preparation cancellation and changed source file after preparation. Use temp files and fake engine transports.
- [ ] Perform preparation on a blocking worker without holding engine locks. Keep bounded progress/status and recoverable errors. Prevent concurrent preparations from publishing a stale preview.
- [ ] Start resolves only the registry plan; reject arbitrary supplied frame arrays. Failed guard checks keep the preview available until expiry/replacement.

## Task 4: UI, MCP and documentation

**Files:** Create `apps/desktop/src/SavedCanReplayPanel.svelte` and `src/savedCanReplay.ts`; integrate `CanWorkbenchPanel.svelte`, `App.svelte`, `types.ts`, `canWorkbench.ts`; extend Agent `protocol.rs`, `tools.rs`, `mcp.rs`, and existing relevant UI tests.

**Interfaces:** saved-analysis selected source/ID/format -> preparation panel -> preview -> guarded explicit Start. MCP Workbench operation variants `prepare_saved_can_replay`, `inspect_saved_can_replay`, `discard_saved_can_replay`, `start_saved_can_replay`, using registered source ID and preview ID. Typed results carry preview, target requirements and status; `stop_operation` stops preparation or replay and finalizes owned evidence.

- [ ] Test UI range parsing, standard/extended selection, displayed selected/excluded counts, source/effective durations, preparation/replacement errors, disabled Start reasons and persistent Stop.
- [ ] Test locked MCP, source resolution, prepare offline without TX, equivalent UI/MCP summary, stale generation, cancellation and prevention of supplied payloads.
- [ ] Add actual UI wiring; preserve existing saved-analysis and live-replay controls. Start remains explicit and output evidence uses a separate new file.
- [ ] Update architecture, CAN Help, Agent Access, backlog and PROJECT_STATUS with implemented behavior and exact results.
- [ ] Run `cargo test --workspace`, `npm run test:ui` and `npm run build` from `apps/desktop`, changed-file formatting and `git diff --check`. Build the current Windows debug desktop and report its exact path. Exercise preview, Stop and submitted status with a fake transport; physical replay remains a separately authorized acceptance test.
