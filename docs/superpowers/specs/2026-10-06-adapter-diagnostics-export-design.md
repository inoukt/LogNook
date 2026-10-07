# Adapter Diagnostics and Support Export Design

## Goal

Make slow or failed adapter operations diagnosable from a file the user can export and return with a bug report. The current `.lognook` format stores measurements, while operation evidence is split across a few OEM JSONL files; Identify, Connect, provider discovery, and most transport failures are not retained together.

## User-visible result

- A Diagnostics view, reachable from the Settings menu, shows a live, timestamped operation log with adapter/provider, operation stage, elapsed time, outcome, and error details.
- The view shows progress and error messages for provider discovery, Identify, Connect, Disconnect, acquisition, and one-shot research operations, with elapsed time and the adapter/J2534 stage. It does not show raw request/reply transcripts.
- Provider discovery, Identify, Connect, Disconnect, and one-shot research operations are always logged. Acquisition progress is summarized by signal ID, attempt/success/error counts, observed rate and latency; sample values and packet payloads are not copied into the new diagnostics log.
- **Export support bundle** opens the native save dialog and creates one create-new `.zip` file. It contains environment/app metadata, the bounded diagnostics log, and existing OEM evidence JSONL files. The user may select a `.lognook` recording to include; the default is no recording.

## Diagnostic detail needed

- Every top-level operation has start, progress-stage, and terminal success/error events with wall time and duration. Preserve the exact adapter/provider identity and exact error text/code.
- Identify and Connect expose adapter-specific stages so a slow 20-second operation is attributable to a particular step. A30 stages include Bluetooth session open, relay/identity handshake, announcement, and supported-PID discovery. SX/ELM stages include serial or Bluetooth open, adapter identity/initialization, CAN or protocol setup where applicable, and supported-PID discovery. OpenPort stages include provider listing/diagnostics, DLL resolve/load, J2534 open/version, channel connect, filter setup, Mode 01 discovery, and selected PID reads.
- For each selected PID, report requested/supported state, sample successes, first-response delay, observed response rate, and response latency summary. A failed grouped poll records the full requested PID set, duration, transient/fatal status, exact adapter error, and consecutive-failure count; attribute a failure to one PID only when the adapter error identifies that PID. Values and raw payload bytes are omitted; a selected `.lognook` recording can provide measurements when needed.

## Data flow

1. Tauri command boundaries record operation start, progress, completion, duration, adapter/provider identity, selected or supported signal IDs when available, returned capabilities, and exact error text. Provider and J2534 worker stages remain distinct so an OpenPort failure can be located at discovery, library open, version read, connect, filter setup, or message read.
2. Acquisition maintains per-signal success counters and response timing from samples on the existing adapter owner. It emits an aggregated progress snapshot at most once per second and a final summary on disconnect. Poll failures are logged with the requested signal set and exact adapter error; individual errors remain visible immediately. It never emits or persists packet payloads into the diagnostics stream.
3. A bounded diagnostics writer appends structured JSONL records to `app_data_dir()/diagnostics/operations.jsonl`. Live events are forwarded to the Diagnostics view. If diagnostics storage is full or unavailable, the app reports that condition and continues acquisition without delaying or dropping measurement samples.
4. Export collects the operation log, app/build/OS metadata, and existing OEM evidence files. If the user selects a recording, the export includes that exact session file. Export uses create-new behavior and reports cancellation or I/O errors in the UI.

## Event data

Each event has a schema version, event ID, UTC timestamp, elapsed duration when applicable, operation/session correlation ID, adapter family and stable ID, operation/stage, outcome, selected/supported signal IDs when available, optional per-signal success counts, first-response delay, rate/latency summaries, batch failure classification and consecutive-failure count, and optional error code/detail. Bundle metadata includes app version and OS name/version/architecture. Unsupported fields stay absent rather than being guessed. The new log has no request or response payload fields.

The diagnostics log does not duplicate measurement samples or raw packet transcripts. Measurements stay in `.lognook` or exported CSV. Device identifiers and existing OEM evidence remain available in the support bundle. The Diagnostics view lets the user inspect included events before export.

## Bounds and performance

- Persisted diagnostics are size-bounded and rotated; the live view retains only a bounded recent-event window.
- Progress is emitted at operation boundaries and as at-most-once-per-second per-signal aggregates, never byte-by-byte. This keeps the live log useful without duplicating high-rate measurement traffic.
- File writes run away from graph rendering and measurement publication. Diagnostics are lower priority than samples; on queue saturation, diagnostic events may be coalesced or counted as omitted, but recorded samples are never dropped.
- Logging is observational. It never starts discovery or acquisition by itself and never adds a vehicle request.

## Tests

- Structured operation logs preserve adapter, operation stage, timestamps, duration, success/error outcome, selected/supported signal IDs, and bounded rate/latency summaries without storing packet payloads.
- Identify/connect success and failure paths both append usable operation records; OpenPort worker failures retain their named J2534 stage and error code.
- Log rotation/live-view bounds and diagnostics-write failure behavior remain bounded and do not affect sample delivery.
- Support bundle tests verify environment metadata, included JSONL evidence, optional selected recording, create-new behavior, and cleanup after a failed write.
- UI tests cover the diagnostics progress/error view, filtering, export cancellation, and successful export.

## Non-goals

- No adapter or ECU requests are added or changed for logging.
- No automatic scan, reconnect, or background protocol activity is added.
- No measurement samples are included unless the user explicitly selects a recording.
- This feature does not change ordinary acquisition, OEM validation state, or CAN Workbench transmission policy. The new diagnostics stream does not record raw TX/RX bytes; existing OEM evidence files retain their established formats.

## Open implementation choices

- Use the existing Tauri dialog and native save-path patterns. The current workspace has no ZIP implementation, so use a maintained Rust ZIP crate with only the needed write/compression features for the single-file bundle.
- Route operation events through the existing Tauri command/event boundaries and acquisition event owner. Add adapter-specific stage details only where the existing typed error/event does not already provide them.
