# Deferred feature backlog

Updated 2026-10-19. This is a deferred-candidate list, not an active work queue or implementation commitment. Check [current project status](../PROJECT_STATUS.md) before starting work.

## Current requested planning

- **Diagnostic task presets.** Drafted a top-bar Presets picker with vehicle-aware
  suggestions, four starter families, generic/model/vehicle custom presets, and
  UI/MCP parity through the existing acquisition owner. Planning only; see the
  [draft design](superpowers/specs/2026-10-19-diagnostic-presets-design.md) and
  [implementation plan](superpowers/plans/2026-10-19-diagnostic-presets.md).

## Previously deferred, now implemented — 2026-10-19

- **Subaru SSM-KWP live graphs.** OpenPort 2 has a separate explicit SSM K-Line
  connection mode using J2534 ISO9141 at 4,800 baud. The worker uses typed SSM2
  initialization and address-read operations only, validates response framing,
  length and checksums, groups selected source memory addresses by ECU, and feeds
  decoded or raw-code suspect samples into the shared graph/recording pipeline. FHI-CAN stays
  on the existing OBD/CAN route; KWP14230 remains searchable and unsupported.
  The SSM route interprets the first request-header byte as the ECU address and
  the response-header byte as tool address F0; the extra `$20` in the Engine
  source header remains provenance and is not transmitted. Imported mappings
  remain inferred, `not_tested`, and `auto_request=0`. See
  [Subaru catalog workflow](SUBARU_CATALOG.md). Physical ECU validation remains
  open.

Recent completed work is retained in dated plans, including [CAN discovery, diagnostics and simultaneous adapters](superpowers/plans/2026-10-09-discovery-diagnostics-dual-adapters.md), [continuous OEM logging](superpowers/plans/2026-10-07-continuous-oem-logging.md), and [live CAN investigation](superpowers/plans/2026-10-07-live-can-investigation.md). These plans are historical records; they are not the current requested feature bundle.

## Remaining candidate order

1. **DBC import.** Import licensed/user-supplied DBC definitions into existing CAN fields with ID width, bit numbering, byte order, signedness, scaling, units, and source attribution. Preview mappings before applying them. Report unsupported constructs explicitly; handle multiplexing deliberately rather than silently flatten it. Verify imported fields against captured frames and the Rust decoder.
2. **Recording profile context.** Add optional versioned profile ID/fingerprint/definition references at recording start while keeping existing measurements immutable and old session files readable.

## Recently implemented

- **Subaru SSM-KWP K-Line acquisition.** OpenPort 2's dedicated Subaru SSM mode
  is available from the desktop adapter selector and Agent MCP. Explicitly logged
  SSM-KWP catalog rows use fixed read-only init/address-read commands and the
  shared acquisition graph/recording pipeline. Worker fixtures cover SSM2 packet
  checksums and split/echoed replies; physical vehicle compatibility is not yet
  verified. See [Subaru catalog workflow](SUBARU_CATALOG.md).

- **Saved-capture CAN TX replay.** Prepare a bounded immutable one-ID/time-range RX clip from `.candump.csv`, inspect source/target, then explicitly start one pass through the current CAN owner with separate paired evidence. UI and MCP share preparation/start/cancellation guards. See [CAN workflow](DISCOVERY_DIAGNOSTICS.md#transmit-a-selected-id-from-a-saved-capture).
- **Vehicle/ECU profiles, offline VIN identity, and diagnostic history.** Local versioned identity/selection snapshots, standby recognition, exact/review/conflict matching, explicit eligible Resume preview, guarded signal replacement, provenance-aware full VIN storage, offline WMI/check-digit/year decoding, verified report history, resumable import, and MCP parity are implemented. Unsupported/revised selections remain saved/skipped. Automatic adapter configuration/connection and fuller manufacturer/model decoding remain future work. See [profile and history workflow](VEHICLE_PROFILES.md).

- **Session organisation.** Rename, confirmed Delete, Folder, Share and Save As workflows are implemented; see the 2026-10-09 status entry and tests before proposing a replacement.
- **Selected CAN-ID replay.** Counts/Frames can freeze matching retained RX frames and explicitly transmit one bounded replay sequence in active mode. The Agent MCP exposes the same operation using only the current live connection's validated RX buffer. See [Agent Access](AGENT_ACCESS.md) and the 2026-10-13 status entries.
- **CAN Help and optional hover explanations.** The small `?` beside CAN bus fields opens a compact guide to connection, capture, repeated event discovery, counts/frames, decoding/scopes, triggers, scans, manual frames, and saved analysis. Its persisted **Show control tips** checkbox toggles the CAN control hints, including checkbox explanations. Opening Help does not transmit traffic. Instructions: [CAN Help](DISCOVERY_DIAGNOSTICS.md#can-help-and-control-explanations).
- **Dedicated diagnostic views.** The top-bar vehicle view groups DTC categories, VIN/calibration/ECU name and Mode 06 support/results by responding ECU. SX/ELM/A30/OpenPort/Candlelight OBD and simulator use the shared diagnostic reads and native MCP; unmapped Mode 06 values remain raw. OpenPort Mode 06 and continuous OEM transport parity have software fixtures; broader physical acceptance remains open.
- **CAN discovery workflow.** Live event candidates link changed bytes/bits to the existing field editor, transition evidence, a bounded live/frozen scope and Save and graph. Saved-capture analysis remains available; this new interactive scope uses live recent frames.
- **Simultaneous OBD and CAN.** One OBD source and one RH02/Candlelight source share a session timeline and recording, with independent disconnect/failure handling. Adding a second source during recording still requires stopping first. Instructions: [Discovery and diagnostics](DISCOVERY_DIAGNOSTICS.md).

- **6 — Continuous community OEM logging.** Runs only explicitly selected numeric candidates that are auto-requestable and physically verified for the selected catalog vehicle. Polls on the serialized adapter owner, groups same-route signals into one request, reports actual request/sample/error/latency data, and publishes raw response payloads and numeric samples through the common recording pipeline. Local Subaru graph logging is a separate manually selected research path; it does not change this eligibility rule. Implementation plan: [continuous OEM logging](superpowers/plans/2026-10-07-continuous-oem-logging.md). The bundled community catalog has no eligible entries, so community OEM polling remains unavailable until exact-profile validation evidence is added.

Each item gets a focused implementation plan when selected. Choose dependencies at that point rather than scaffolding them now.
