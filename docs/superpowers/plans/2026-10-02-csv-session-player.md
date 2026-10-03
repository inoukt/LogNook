# CSV Session Player Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Import LogNook CSV exports as durable sessions and play them with seek, speed, marker management, and zoomable graphs.

**Architecture:** Stream typed CSV events into the existing SQLite `.lognook` format, then use one Rust playback controller and sample pipeline for imported and recorded sessions. Tauri commands expose file opening, playback state, markers, and bounded graph windows; Svelte provides the player and time-range UI.

**Tech Stack:** Rust, Tauri 2, Svelte 5/TypeScript, SQLite, the existing `csv` crate, and Tauri's native dialog plugin.

**Spec:** `docs/superpowers/specs/2026-10-02-csv-player-design.md`

## Global Constraints

- Keep full-resolution measurements in SQLite; graph output is display-only decimation.
- Keep the original CSV untouched and never overwrite existing CSV or session files.
- Accept both current sample-only CSV exports and new mixed sample/marker exports.
- Keep `.lognook` schema version 1; marker IDs remain session-local.
- Keep replay, CSV import, and marker writes in Rust; send playback events through `SamplePipeline`.
- Use the existing replay speed range of `0.1x`–`32x`.
- Do not change physical adapter behavior or claim new hardware validation.
- Reuse installed dependencies; add only the Tauri 2 dialog Rust plugin for native file selection.
- Keep handwritten source files focused, near 100–250 lines where practical.

## Review Focus

- **Legacy export with sidecar markers:** test `imports_legacy_marker_sidecar_once` in Task 2; preserve samples and import each sidecar marker once.
- **Malformed or empty input:** test `failed_import_removes_temp_and_preserves_target` and `empty_csv_is_rejected` in Task 2; report the row and leave no partial session.
- **Out-of-order and equal timestamps:** test `equal_timestamp_events_are_retained_and_ordered` in Task 3; retain duplicates in deterministic order.
- **Playback commands during a long event gap:** test `pause_returns_before_pending_delay` and `seek_restarts_at_or_after_target` in Task 4; commands respond without waiting for the original event delay.
- **Extreme graph requests:** test `query_window_clamps_bounds_and_caps_output` in Task 3; clamp the window, cap buckets to 2,000, and retain no more than four samples per signal per bucket.

---

### Task 1: Typed CSV events and backward-compatible parsing

**Files:**
- Modify: `crates/lognook-core/src/csv_io.rs`
- Test: `crates/lognook-core/tests/recording.rs`
- Test: `crates/lognook-core/tests/csv_roundtrip.rs`

**Interfaces:**
- Consume: `RecordedEvent::{Sample, Marker}` from `recording.rs`.
- Produce: `pub(crate) fn visit_csv_events<R: Read>(input: R, visit: impl FnMut(RecordedEvent) -> Result<(), LogNookError>) -> Result<(), LogNookError>`.
- Produce: `export_recording_csv` writes a single ordered event stream with `event_type=sample|marker` rows.
- Preserve: `export_csv(&[Sample], output)` and `import_csv(input) -> Vec<Sample>` for sample-only callers.

- [x] **Step 1: Update the existing failing test** `tests/recording.rs::recording_export_contains_samples_and_markers_inline`; assert one CSV has the typed header, one sample row, one marker row with quoted note intact, and no sidecar file.
- [x] **Step 2: Run** `cargo test -p lognook-core --test recording recording_export_contains_samples_and_markers_inline`; expect the header assertion to fail because the current export writes markers to a separate file.
- [x] **Step 3: Implement** the event-row serializer and streaming visitor. Keep old sample-only files readable when `event_type` is absent. New recording exports write markers inline and no longer create a marker sidecar.
- [x] **Step 4: Run** `cargo test -p lognook-core --test recording recording_export_contains_samples_and_markers_inline` and `cargo test -p lognook-core --test csv_roundtrip`; expect mixed-event export and legacy sample round trip to pass.
- [x] **Step 5: Commit** as `feat: store markers in recording csv`.

### Task 2: Atomic CSV import and session metadata

**Files:**
- Create: `crates/lognook-core/src/csv_import.rs`
- Modify: `crates/lognook-core/src/recording.rs`
- Modify: `crates/lognook-core/src/model.rs`
- Modify: `crates/lognook-core/src/lib.rs`
- Test: `crates/lognook-core/tests/csv_import.rs`

**Interfaces:**
- Consume: `visit_csv_events` from Task 1 and `RecordingWriter`.
- Produce: `pub fn import_csv_to_recording(source: &Path, target: &Path) -> Result<RecordingSummary, LogNookError>` in `csv_import.rs`; stream into a temporary sibling session, integrity-check it, atomically create `target` without replacement, then remove the temporary path.
- Produce: `pub fn inspect_recording(path: &Path) -> Result<RecordingInfo, LogNookError>` where `RecordingInfo` has `sample_count`, `duration_ns`, and distinct `{ id, unit }` signals. Task 3 adds marker IDs/list to this type.
- Preserve: existing `.lognook` schema version 1 and its `samples_by_time` index.

- [x] **Step 1: Add failing tests** `imports_sample_only_csv_without_event_type`, `imports_legacy_marker_sidecar_once`, `inline_markers_take_precedence_over_sidecar`, `unknown_event_type_reports_row`, `empty_csv_is_rejected`, `failed_import_removes_temp_and_preserves_target`, and `inspect_recording_reports_duration_and_signals`. Assert sample/marker counts, timestamps, durations, signal IDs/units, row-number errors, temp cleanup, and byte-for-byte preservation of an existing target.
- [x] **Step 2: Run** `cargo test -p lognook-core --test csv_import`; expect compilation to fail because the import and metadata APIs are missing.
- [x] **Step 3: Implement** streaming import through `RecordingWriter`; use a temporary path and clean it on parse, write, integrity, or publication failure. Publish with a same-directory hard link so an existing target cannot be replaced. Import a legacy sidecar only when the main file has no inline markers.
- [x] **Step 4: Run** `cargo test -p lognook-core --test csv_import`; expect all import cases to pass and no partial target to remain after failure.
- [x] **Step 5: Commit** as `feat: import csv as lognook sessions`.

### Task 3: Marker persistence and bounded graph-window queries

**Files:**
- Modify: `crates/lognook-core/src/model.rs`
- Modify: `crates/lognook-core/src/recording.rs`
- Modify: `crates/lognook-core/src/pipeline_worker.rs`
- Modify: `crates/lognook-core/src/lib.rs`
- Create: `crates/lognook-core/src/graph_window.rs`
- Modify: `crates/lognook-core/src/recording_view.rs`
- Test: `crates/lognook-core/tests/recording.rs`
- Test: `crates/lognook-core/tests/graph_window.rs`

**Interfaces:**
- Produce: session-local `Marker.id: Option<i64>`; stored markers have `Some(id)`, while new in-memory markers start as `None`. `RecordingWriter::add_marker(&Marker) -> Result<i64, LogNookError>` returns the database ID.
- Produce: `update_recording_marker(path: &Path, id: i64, label: &str, note: &str) -> Result<Marker, LogNookError>` and `delete_recording_marker(path: &Path, id: i64) -> Result<(), LogNookError>`; unknown IDs return an error.
- Produce: `query_recording_window(path: &Path, signal_ids: &[String], start_ns: i64, end_ns: i64, max_buckets: usize) -> Result<Vec<Sample>, LogNookError>`; clamp time bounds to `[0, duration_ns]`, return empty when end precedes start, cap buckets to 1–2,000, and return ordered first/min/max/last samples per signal bucket.
- Produce: include markers with IDs from SQLite in `RecordingInfo`; IDs are not serialized to CSV.

- [x] **Step 1: Add failing tests** `marker_ids_are_unique_and_crud_persists`, `equal_timestamp_events_are_retained_and_ordered`, `query_window_keeps_extrema_and_caps_output`, and `query_window_clamps_bounds_and_caps_output`. Assert marker edits/deletes survive reopening, sample-before-marker ordering at equal times, at most four points per signal per bucket, retained minima/maxima, end-before-start returns empty, and requested buckets are clamped to 1–2,000.
- [x] **Step 2: Run** `cargo test -p lognook-core graph_window`; expect missing methods or assertion failures.
- [x] **Step 3: Implement** marker transactions and a streaming window scan that retains only each signal's first, minimum, maximum, and last sample per bucket; return results sorted by timestamp and stable database order.
- [x] **Step 4: Run** `cargo test -p lognook-core`; expect CRUD, equal-timestamp ordering, and bounded-window tests to pass.
- [x] **Step 5: Commit** as `feat: query and edit recorded sessions`.

### Task 4: Controllable playback engine

**Files:**
- Modify: `crates/lognook-core/src/engine.rs`
- Modify: `crates/lognook-core/src/pipeline.rs`
- Modify: `crates/lognook-core/src/pipeline_worker.rs`
- Modify: `crates/lognook-core/src/model.rs`
- Create: `crates/lognook-core/src/session.rs`
- Create: `crates/lognook-core/src/playback.rs`
- Test: `crates/lognook-core/tests/playback.rs`
- Test: `crates/lognook-core/tests/pipeline.rs`

**Interfaces:**
- Produce: serialized `PlaybackState { session_id, position_ns, duration_ns, speed, playing }` and session-tagged playback sample/marker events; emit control-state changes immediately and progress at most 20 times per second.
- Produce: `LogNookEngine::load_recording(path: &Path) -> Result<RecordingInfo, LogNookError>`, `start_replay(speed: f64) -> Result<(), LogNookError>`, `pause_replay() -> Result<(), LogNookError>`, `resume_replay() -> Result<(), LogNookError>`, `seek_replay(position_ns: i64) -> Result<(), LogNookError>`, and `set_replay_speed(speed: f64) -> Result<(), LogNookError>`.
- Produce: `add_playback_marker(label: &str, note: &str) -> Result<Marker, LogNookError>`, `update_playback_marker(id: i64, label: &str, note: &str) -> Result<Marker, LogNookError>`, and `delete_playback_marker(id: i64) -> Result<(), LogNookError>`.
- Preserve: `replay_last(1.0)` behavior as a wrapper over the shared playback controller.
- Consume: Task 3 marker and session APIs. Marker add/edit/delete operate on the active session at the playback position.

- [x] **Step 1: Add failing tests** `pause_returns_before_pending_delay`, `seek_restarts_at_or_after_target`, `speed_change_updates_pending_delay`, `invalid_replay_speed_does_not_change_state`, and `playback_marker_persists_at_playhead`. Assert pause/seek complete before a 500 ms sample gap ends, seek begins at the first event at or after its target, speed changes scale the remaining wait, invalid speeds leave state unchanged, and a marker reloads with the current position.
- [x] **Step 2: Run** `cargo test -p lognook-core`; confirm the current stop-only worker fails the new controls.
- [x] **Step 3: Implement** a command channel for playback control. Poll it during event delays; seek from the indexed timestamp; serialize marker writes by stopping and releasing the reader, committing, then resuming at the same position when needed.
- [x] **Step 4: Run** `cargo test -p lognook-core`; expect control, event-order, and marker tests to pass.
- [x] **Step 5: Commit** as `feat: control recording playback`.

### Task 5: Tauri import and player commands

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Create: `apps/desktop/src-tauri/src/commands/player.rs`

**Interfaces:**
- Produce: `open_csv` opens a native `.csv` picker, returns no change on cancel, imports to a unique `recordings_dir` session, then loads it into the engine.
- Produce: a plain helper `import_selected_csv(selected: Option<PathBuf>, engine: &mut LogNookEngine, recordings_dir: &Path) -> Result<Option<RecordingInfo>, String>` so cancellation and selection failure are testable without opening a native dialog.
- Produce: Tauri commands for last-recording load, play/pause/resume/seek/speed, marker add/edit/delete, bounded graph windows, and selected values at the playhead.
- Consume: Task 2 `import_csv_to_recording`/`inspect_recording`, Task 3 marker/window APIs, and Task 4 playback methods.

- [x] **Step 1: Add a failing test** `open_csv_cancel_returns_without_changing_session` for a plain helper that accepts an optional selected path; assert `None` returns `None` and leaves the engine's current session path unchanged.
- [x] **Step 2: Run** `cargo test -p lognook-desktop open_csv_cancel_returns_without_changing_session`; expect the command helper to be missing.
- [x] **Step 3: Implement** the Tauri Dialog Rust plugin, native picker, selection boundary, and typed commands; do not pass file contents through frontend IPC.
- [x] **Step 4: Run** `cargo test -p lognook-desktop` and `cargo check --workspace`; expect both to pass.
- [x] **Step 5: Commit** as `feat: expose csv player commands`.

### Task 6: Svelte player and graph viewport

**Files:**
- Create: `apps/desktop/src/PlaybackPanel.svelte`
- Modify: `apps/desktop/src/App.svelte`
- Modify: `apps/desktop/src/SignalGraphs.svelte`
- Modify: `apps/desktop/src/LiveChart.svelte`
- Modify: `apps/desktop/src/types.ts`
- Modify: `apps/desktop/src/workspace.ts`
- Modify: `apps/desktop/src/app.css`
- Test: `apps/desktop/tests/workspace.test.mjs`

**Interfaces:**
- Consume: Task 5 Tauri commands and `PlaybackState`, `RecordingInfo`, `Marker`, and `Sample` types.
- Produce: `formatDurationNs(value: number): string`, `clampSeekPosition(positionNs: number, durationNs: number): number`, `zoomTimeWindow(range, anchorNs, factor, durationNs)`, and `panTimeWindow(range, deltaNs, durationNs)` in `workspace.ts`.
- Produce: `orderMarkersForTimeline(markers)` for a stable marker time order before rendering timeline ticks and rows.
- Produce: a player panel with open CSV, play/pause, seek/time, speed, add/edit/delete/jump-to-marker controls, zoom in/out/reset, and shared pan.
- Produce: identical time bounds for combined and separate graphs; query Task 3's bounded data whenever signals or viewport change.
- Produce: ignore session-tagged replay events after the active session changes; refresh readouts on a paused seek.
- Produce: dark-slate scrollbar tracks and teal thumbs/seek controls shared with the PID list.

- [x] **Step 1: Add failing UI helper tests** `formats_playhead_time`, `clamps_seek_to_recording_duration`, `zoom_and_pan_stay_inside_recording`, and `orders_markers_for_timeline`; assert `61_234_000_000` ns formats as `01:01.234`, seek remains within `[0,duration]`, zoom preserves its anchor and pan preserves window width, and marker ticks are time-ordered.
- [x] **Step 2: Run** `npm run test:ui` from `apps/desktop`; confirm the new helpers are missing.
- [x] **Step 3: Implement** the player panel and app wiring, including a return-to-start button and marker ticks/vertical graph lines. Keep the current session active if import fails; load imported session signals without fabricating PIDs; keep live values and selection behavior intact.
- [x] **Step 4: Run** `npm run test:ui` and `npm run build`; expect UI tests to pass and Svelte/Vite to report no errors.
- [x] **Step 5: Commit** as `feat: add csv recording player ui`.

### Task 7: Documentation and full verification

**Files:**
- Modify: `PROJECT_STATUS.md`
- Modify: `LogNook_Blueprint/02_ARCHITECTURE.md`

- [x] **Step 1: Update** status and architecture docs with the final CSV format, backward-compatibility behavior, player controls, and tested limits.
- [x] **Step 2: Run** `cargo fmt --all -- --check`, `cargo test --workspace`, `cargo check --workspace`, `cargo build -p lognook-desktop --features tauri/custom-protocol`, `npm run test:ui`, and `npm run build`.
- [x] **Step 3: Commit** as `docs: record csv player status`.

## Completion Contract

- All seven task commits are present and the worktree is clean.
- Rust, UI, formatting, desktop build, and production Svelte build checks pass.
- No source CSV is modified; new exports contain samples and markers in one CSV; existing sample-only files and legacy marker sidecars import.
- Physical OBDLink SX validation remains separate and is not claimed by this work.
