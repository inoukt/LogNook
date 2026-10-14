# Saved CAN replay and Vehicle/ECU profiles — proposed design

Status: implemented in software on 2026-10-14. Recording profile-context metadata remains deferred. Prepared against `e6439a53966a204515b2a5cec61332c74cb2752b` on 2026-10-13.

## Intended outcome and owner choices

The owner wants to transmit a selected CAN ID from a saved capture and resume a vehicle's signal selections when the same vehicle/ECU is connected again. The owner selected **A: show a one-click Resume selection with a preview**. Recognizing a vehicle must therefore present a filtered preview; applying the saved selection requires Resume.

Deliver these as two independent features. Saved replay extends the existing CAN Workbench. Profiles add persistent application state and a staged recognition/resume flow. Neither feature changes the authoritative PID catalog or creates vehicle validation from a response.

## Existing behavior to reuse

- `CanCaptureReader` streams LogNook's eight-column `.candump.csv` format and caps physical lines at 256 bytes.
- `CanWorkbench` owns serialized CAN I/O. Live ID replay accepts 1–400 received classic frames, preserves captured intervals with a 1 ms floor, rejects gaps over 60 seconds, and records source/TX evidence.
- Saved CAN analysis exists in `commands/can.rs`; it provides census and optional paired JSONL metadata. Saved files are currently analysis sources only.
- `AgentLogBuffer` exposes saved sources and current live data. MCP replay currently resolves the live buffer server-side.
- `App.svelte` and `workspace.ts` persist global log/graph IDs and workspace preferences in localStorage.
- OBD connection discovers support and immediately starts selected polling. `AcquisitionHandle` currently has no standard-signal replacement command.
- `DiagnosticReport` retains per-responder VIN, calibration ID and ECU-name reads on SX, ELM327 and A30. OpenPort does not yet support that diagnostic interface. Identity-only reads need a narrow query path; running the whole DTC/Mode 06 snapshot just to recognize a vehicle would add unnecessary work.
- The bundled v0.3 catalog already contains research vehicle records and validation gates. Those records are different from local connection/profile preferences.

## Alternatives considered

1. **Recommended:** bounded immutable saved replay previews; local SQLite profiles; staged identity verification and explicit Resume. Extends current owners and keeps source and target evidence clear.
2. Replay whole files as an unbounded stream and restore selections directly from global preferences. Lower initial UI effort, but introduces file mutation, cancellation and cross-vehicle selection risks without a reviewable clip.
3. Build a general transport replay engine and cloud profile manager. Adds dependencies and responsibilities beyond these requests. Defer until there is a concrete need.

## Saved CAN capture replay

### User flow

1. Open a saved `.candump.csv` in the existing saved-capture analysis section.
2. Choose one exact CAN ID and its 11-/29-bit format, then set Start and End in source elapsed time. Offer **Prepare TX replay** beside the selected census row.
3. Review the prepared source: filename, selected ID/format, original row/time range, matching RX count, excluded TX/flagged counts, captured span, effective replay duration, and known source bitrate.
4. Connect the intended Candlelight target in active mode and start a separate paired output capture. The preview shows the actual target identity, channel and bitrate.
5. Choose **Start TX replay · once**. Display submitted-frame progress and persistent Stop. Stop/capture close/disconnect/Agent lock/error cancel remaining transmissions.
6. Finish the output capture. Original source bytes and timestamps remain untouched.

### Source and bounds

Version 1 accepts LogNook `.candump.csv` files, one CAN ID/format, unflagged classic RX data frames, and one pass at original intervals with the existing 1 ms minimum and no catch-up burst. Saved replay is independent of the live ring's 400-frame limit.

Proposed bounds: input file at most 256 MiB; prepared selection 1–10,000 frames; source span and effective paced duration at most 600 seconds each; each original gap at most 60 seconds. All arithmetic is checked. Equal timestamps use file row order and the 1 ms floor. Over-limit selections are rejected with a request to narrow the range; frames are never silently truncated. Live replay retains its existing 400-frame cap.

Preparation streams and validates the complete file on a blocking worker, with cancellation checks between batches. Any malformed row rejects preparation before transmission, even if the row is outside the selected range. Valid TX and flagged rows are excluded and counted. Out-of-order timestamps within the matching sequence reject the clip rather than silently reordering it. Preserve original physical row numbers.

The backend owns the prepared frame snapshot and a SHA-256 digest of the source file. Start accepts an opaque preview token rather than caller-supplied frames. Later file edits cannot change a prepared sequence. One preview per app, expiry after 10 minutes, replacement invalidates the old token. Selecting/connecting/launching does not start replay.

Use existing source-sidecar parsing for optional adapter/bitrate metadata. Missing sidecar permits a preview with unknown source bitrate. A known source/target bitrate mismatch rejects Start; the operator explicitly corrects the target or chooses another capture. Never infer vehicle identity or change bus settings from a file. Reject a source file that is currently being written by LogNook and refuse input/output path aliases.

### Shared worker and evidence

Add a typed prepared replay plan with source provenance and validated frames. Keep the current live API as a compatibility wrapper. Both source kinds use the existing worker, TX progress, echo tracking and mutual exclusion with scans/manual sends.

Capture target connection generation at Start and verify it atomically with enqueue. A reconnect to the same adapter is a different generation. Evidence records source kind, SHA-256, original row/time range, selected ID/format, source bitrate if known, actual target/channel/bitrate and replay bounds. Journal source frames in chunks of at most 400 and flush preparation evidence before the first TX. Preserve submitted counts on later evidence failure; never report USB submission or echo as ECU acceptance.

MCP adds prepare/inspect/discard/start saved-replay operations. Source references resolve to saved sources listed by the application, never arbitrary payload arrays. UI-picked external files can be registered as saved sources in the current app session. Preparation is available without an adapter; Start requires unlocked Agent Access and the same active-mode/evidence/owner rules. Stop uses `stop_operation`.

## Vehicle and ECU profiles

### Identity and matching

Store a named local vehicle profile separately from its ECU variants. VIN is a vehicle identity; protocol/address/calibration identify an ECU variant. Adapter serial, provider, COM port, bitrate, make/model/year and ECU name alone do not prove a vehicle match.

Fingerprint version 1 uses a validated 17-character VIN digest plus per-ECU protocol, explicit request/response addressing and observed calibration/software identifiers when available. Canonical encoding is versioned, structured and sorted. VIN digest is pseudonymous, not guaranteed anonymization. Persist a masked VIN for display; raw diagnostic evidence retains its existing policy. Do not invent VIN decoding or unavailable CVN/software reads.

Match states: `exact`, `review_required`, `identity_unavailable`, `conflict`. A field that was previously observed but is now missing cannot be assumed equal. Conflicting VIN or calibration blocks exact matching. Extra newly discovered modules do not erase existing module records; changed or absent modules are handled individually. Different adapters can match the same vehicle. Simulator and CAN-only connections have separate explicit profile scopes and cannot inherit live-vehicle OEM eligibility.

A manually named profile helps when identity reads are unavailable (including current OpenPort). It is a user-selected association, not an exact fingerprint match. Generic selections still require fresh current-connection support. OEM auto-polling requires the existing exact catalog vehicle/ECU validation gates.

### Profile contents and storage

Use one versioned `app_data_dir()/profiles.sqlite` application-state store through a small Rust service. This is not a competing PID catalog. Use the existing rusqlite dependency; keep the research database read-only.

Tables cover vehicle profiles, per-ECU variants, observed identity/support history and versioned selection snapshots. Store adapter preferences as hints, catalog vehicle association when explicitly established, log IDs, graph IDs, derived definitions/dependencies, CAN field references and their revisions, and selected OEM candidate/source/decoder revisions. Preserve exact routes and definition provenance. Measurements remain in their existing recordings/captures.

Save/Update profile is explicit. Partial refresh failure appends an observation and retains the previous verified snapshot. Skipped selections remain saved, so a temporary ECU timeout or unsupported adapter does not destroy the preset. Profile writes are transactional with schema-version checks; malformed/newer stores fail visibly without being replaced. First profile creation may offer to copy current global selections; global preferences remain the fallback for users without profiles.

### Recognition and resume flow

First visit: connect normally, explicitly recognize identity, select signals, and choose **Save vehicle profile**.

Later visits: with vehicle profiles enabled, Connect discovers support and keeps the serialized acquisition worker in standby. Read supported identity queries through that same owner; do not start the previous vehicle's selected polling before recognition. Ordinary connection without profiles retains its current flow.

Show the recognized profile and **Resume selection** preview: eligible logging/graph channels, unsupported channels, missing dependencies, stale catalog/decoder revisions, and ambiguous/blocked ECU matches. Identification errors leave the worker available for ordinary generic selection; they do not erase the profile or prevent generic OBD use.

Resume atomically applies the eligible set through a new serialized selection-replacement command. Fresh generic support and adapter capabilities are required. Derived signals require current compatible dependencies. Research-only OEM channels stay graph-only; continuous OEM channels must pass the existing auto_request, vehicle-validation, applicability and exact-source checks again. A saved CAN field requires the intended active CAN source/context; restoring metadata never starts CAN TX.

If nothing is eligible, present the skipped reasons and let the user pick supported signals. Recording, playback, research, scans and replay must be stopped before applying a profile in version 1. Cancelling the preview leaves the current working selection intact. A connection-generation and profile-revision check prevents late identity reads or an old Resume action from affecting a newer connection/profile.

Backend state is authoritative for profile resolution and application. Publish one selection update event for the UI and MCP; MCP cannot apply selections through a weaker validation path. Profile recognition never starts recording, CAN replay, scans or state-changing diagnostics.

Deferred from v1: attaching the active profile ID/revision, fingerprint version/digest, ECU variants and definition references to a recording. Existing recordings remain unchanged and cannot be retroactively relabeled. Profile metadata is stored only in `profiles.sqlite` until recording-context support is implemented and tested.

### MCP parity

Typed operations: list/get/save/update profiles, recognize current connection, preview resume, apply resume and discard preview. Responses include match state, observed identity freshness, eligible/skipped selections and reasons, profile revision and connection generation. Unlock applies to mutations and hardware operations using existing access rules; locking cancels queued recognition/resume work. Identity-only reads use the adapter queue, deadlines and evidence path. `get_status` exposes current profile and standby/resume state.

## Delivery order and acceptance

1. Saved-capture replay: preparation/core validation, worker/evidence integration, UI/MCP workflow and software acceptance.
2. Profiles: persistent model/matching, standby and serialized signal replacement, recognize/resume UI/MCP, and acceptance. Recording metadata remains deferred.

Software acceptance uses fake transports, crafted captures and identity fixtures. Hardware acceptance is a separate explicitly started test: selected known bench frames for replay; Ford and Scion kept as distinct identities for profiles. Do not describe software tests as physical validation.

Plans: [saved capture replay](../plans/2026-10-13-saved-can-capture-replay.md) and [profiles and resume](../plans/2026-10-13-vehicle-ecu-profiles-resume.md).
