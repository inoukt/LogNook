# CSV Session Player Design

**Status:** Approved; implementation complete

**Date:** 2026-10-02

## Goal

Open LogNook CSV exports as replayable sessions, control playback from the
workspace, add and manage timestamped markers, and inspect the full recording
with zoomable, pannable graphs. Existing `.lognook` recordings use the same
player. The imported source CSV remains unchanged.

## User flow

1. When disconnected and not recording, choose **Open CSV**. A native file
   picker selects one `.csv` file.
2. LogNook validates and streams the file into a new local `.lognook` session.
   A failed or cancelled import leaves the current session alone and removes
   any incomplete temporary session.
3. The imported session loads paused at the beginning. The signal picker lists
   signals found in the session. Known signal IDs reuse the existing catalog;
   unknown IDs display their stored key and unit without inventing a PID.
4. A shared player controls both imported CSV sessions and LogNook recordings:
   play/pause, seek, elapsed and total time, playback speed, and return to the
   beginning. The slider uses the current playhead and shows marker ticks.
5. Add a marker at the playhead with a label and optional note. Markers can be
   selected to seek, edited, and deleted. Marker changes are saved in the
   active `.lognook` session and included in the next CSV export.
6. Graph controls zoom in, zoom out, reset, and pan through a shared time
   window. Combined and separate graph layouts show the same time range and
   marker positions.

Opening a CSV or switching sessions is disabled during live acquisition or
recording. Opening a new session stops the current replay only after the new
session has imported successfully.

## CSV format and compatibility

New exports use one event table with these columns:

```text
event_type,elapsed_ns,wall_time_unix_ns,source_id,ecu_id,signal_key,value,unit,quality,origin,raw_hex,label,note
```

- `event_type=sample` rows retain the existing measurement fields.
- `event_type=marker` rows contain elapsed time, wall time, label, and note;
  measurement-specific fields are empty.
- An empty marker note is valid. Cross-kind fields are rejected, and elapsed
  times must be non-negative.
- The importer continues to accept the current sample-only format, which has
  no `event_type` column.
- Old `<name>-markers.csv` sidecars are imported when the main file has no
  inline marker rows. New exports write markers inline and stop producing a
  sidecar.
- Marker IDs are session-local. Rust returns them to the UI as opaque handles
  for edit and delete actions, but does not write them to CSV. Import assigns
  new IDs.
- Unknown event types, invalid values, or malformed rows fail the import with
  a row-specific error. No partial session becomes visible.

The selected source CSV is read-only. Imported events are written to a unique
session under the existing app recordings directory. Exporting that session
creates a new CSV with both samples and markers; it never overwrites the
source. Existing CSV export naming and no-overwrite behavior remain in force.

## Architecture and data flow

### Import and persistence

- Use Tauri 2's native dialog plugin to select a CSV path and a Rust command to
  stream it through the existing CSV parser. This avoids copying a potentially
  large file through frontend IPC. The plugin is not currently installed; add
  only its Rust dependency and initialize it in the Tauri builder.
- Reuse `RecordingWriter` and the current SQLite session schema. Import into a
  temporary path, validate all rows, finish and integrity-check the database,
  then atomically create the unique final path without overwriting anything.
  Remove the temporary link after promotion; if the filesystem cannot safely
  publish the completed database, fail and remove the temporary files.
- Keep the source CSV untouched. Import legacy sidecars only when no inline
  markers are present, preventing duplicate annotations.
- Export events in timestamp order from the session database. Marker IDs stay
  scoped to the SQLite session; CSV marker rows store portable marker content,
  and IDs are assigned again when imported.

Tauri's official Dialog plugin provides native file selectors on Windows,
Linux, and macOS and exposes the selected path to Rust. The implementation
will follow the [Tauri 2 Dialog plugin guide](https://v2.tauri.app/plugin/dialog/).

### Playback

- Replace the stop-only playback worker with one controller that accepts
  play/pause, seek, speed, and stop commands. Keep all file reading and timing
  in Rust; send samples and marker events through the current
  `SamplePipeline`.
- Seek from the selected timestamp using the existing time indexes. A seek
  updates the playhead immediately; play resumes from that point. Speed changes
  apply to the remaining interval without changing stored timestamps.
- Emit bounded playback-state updates with position, duration, speed, and
  paused/playing state. Validate speed with the existing `0.1x`–`32x` engine
  limit.
- Tag replay events with the active session ID so queued events from a replaced
  session can be ignored. A paused seek refreshes the current values from the
  latest sample at or before the playhead.
- Serialize marker edits with playback reads: pause and release the reader,
  commit the marker transaction to the active session, then resume from the
  same playhead if playback was running. A write failure leaves the marker
  unchanged in both the database and UI.

### Graph history and interaction

- Keep full-resolution samples in SQLite. Do not send an entire large session
  to Svelte.
- Add a time-window query for selected signals. Return a bounded, ordered
  min/max envelope per pixel bucket so spikes remain visible while zooming and
  panning; this is display decimation only and never changes stored samples.
- Refresh the graph window when the playhead, zoom, pan, or signal selection
  changes. Reuse `LiveChart` and the existing combined/separate graph modes.
- Define a shared dark-slate scrollbar style for the PID list and player
  timeline, with the existing teal accent for the thumb and seek slider.

## Files likely to change

- `crates/lognook-core/src/csv_io.rs`, `recording.rs`, and `model.rs`: typed
  event rows, legacy CSV/sidecar import, atomic session import, marker IDs,
  and mixed-event export.
- `crates/lognook-core/src/engine.rs`, `pipeline.rs`, and
  `pipeline_worker.rs`: controllable playback, playhead state, marker CRUD,
  and time-window reads.
- `apps/desktop/src-tauri/Cargo.toml`, `lib.rs`, and `commands.rs`: the native
  dialog plugin plus import, playback, marker, and graph-window commands.
- `apps/desktop/src/App.svelte`, graph components, `workspace.ts`, `types.ts`,
  and `app.css`: player controls, marker editing, time-window controls, and
  shared styling.
- Add focused tests for old/new CSV formats, marker round trips, atomic import,
  playback control, graph-window bounds, and the UI's player state.

## Error handling and data safety

- Cancellation is a no-op. Invalid or unreadable CSV reports the row and reason
  and preserves the current player session.
- New session files use unique names. Imports and exports do not overwrite
  existing files.
- A marker edit is acknowledged only after its SQLite transaction commits.
- Playback stop, end-of-file, and errors release the reader and update the
  player state. Seeking or changing speed does not rewrite measurement times.
- The native dialog selects a path; the Rust import command still validates the
  selected file and parses its contents before accepting it.

## Out of scope

- Mapping third-party CSV formats, DBC files, or arbitrary column layouts.
- Editing the original CSV in place.
- A session browser, cloud sync, installer, new graph types, or hardware
  connection changes.
- Changing physical acquisition or SX polling; vehicle validation remains a
  separate task.

## Acceptance criteria

1. A current LogNook CSV and an older sample-only CSV both load into a local
   `.lognook` session; a legacy marker sidecar is preserved when applicable.
2. A new CSV export contains sample and marker event rows in one file and can
   be imported again without losing timestamps, values, raw bytes, labels, or
   notes.
3. The player can pause, resume, seek, change speed, and return to the start;
   added, edited, and deleted markers persist and follow the playhead.
4. Zoom, reset, and pan affect the visible time range in both graph modes.
   Marker positions appear on the timeline and graph. The graph query stays
   bounded while stored sample detail remains intact.
5. Invalid imports leave no partial session and do not interrupt the current
   session. Source CSVs are never modified or overwritten.
6. Rust and UI checks pass, and the player controls and scrollbar use the
   LogNook dark-slate and teal theme.
