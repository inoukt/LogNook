# CAN event freeze-frame comparison

**Status:** Draft for review  
**Date:** 2026-10-09

## Goal

Help an operator see which CAN messages recur around repeated, manually marked actions, and whether each message payload is exact or only bit-similar across event windows. This is raw traffic evidence only; it does not decode or assign meaning to CAN data.

## Agreed operator behavior

- The operator marks the same action more than once while a paired CAN capture is active.
- Each mark uses the existing configured pre/post window. When that window completes, LogNook records a compact snapshot for that event.
- An in-app comparison dialog opens from a labeled group in the Events view. It compares standard and extended IDs separately, within that label only.
- The dialog shows each retained event as a column and CAN IDs as rows. Each cell shows the latest RX payload in that event window and the number of RX frames observed for that ID.
- Exact recurring payloads are green, with stronger green for greater recurrence across completed windows. A different payload with the same ID and DLC is marked similar when at least 90% of its bits match the most common exact payload for that ID. Other payloads are neutral. Missing, pending, incomplete, and memory-omitted observations are distinguishable.
- The dialog is resizable, scrolls in both directions, and can be closed without stopping capture. It uses the current frozen/live event snapshot just as the existing event ranking does.

## Approaches considered

1. **Compact snapshot per marked window (selected).** Keep the latest RX frame and count per CAN ID for each completed mark. This gives the UI per-event evidence while keeping memory bounded and avoids retaining a second full copy of the raw capture.
2. **Retain every frame per event for the comparison.** This duplicates potentially large raw traffic windows and is unnecessary because the existing raw capture remains the complete evidence source.
3. **Color only the aggregate event ranking.** This cannot show which payload appeared at each mark, so it does not meet the requested freeze-frame comparison.

## Data and bounds

Extend the existing `CanLiveEventSnapshot` event-group contract with ordered per-mark window summaries. Each summary carries a stable capture-local sequence, the mark time and configured pre/post durations, a `pending`/`complete`/`incomplete` state, a count of IDs omitted by the memory cap, and zero or more per-ID message summaries. A message summary carries the CAN ID and width, RX frame count, and latest exact RX `CanFrame` from that window.

Use existing capture bounds: at most 256 marked events per analysis, 32 pending windows, and 10,000 frames per event window. Retain at most 100 distinct IDs per completed window, ranked by RX frame count descending with CAN ID/width as deterministic tie-breakers. The latest frame and count are sufficient for the visual comparison; the raw capture remains the full frame history. A window with more than 100 IDs reports the omitted count. Do not report an ID as absent in a window whose ID list was capped; show it as not retained/unknown instead.

A window that exceeds the existing frame cap, loses required pre-window history, or remains pending when capture stops is marked incomplete and has no comparable payload cells. A completed empty window remains complete with no IDs. Clearing event results clears these live summaries together with the current live ranking; it does not alter saved raw frames or event marks.

The additional worst-case retained payload is 25,600 frame summaries (100 IDs × 256 marks), each containing at most eight classic-CAN data bytes. Snapshots are published on mark/window state changes, not per received frame.

## Comparison rules

- Aggregate by `(CAN ID, standard/extended width)` within one event label.
- For each ID, select the most frequent exact payload among completed, retained windows as the comparison payload. Break ties by earliest event. Require at least two completed observations before applying recurrence colors.
- For each ID, the recurrence denominator is completed windows where the ID is present or where the full ID list was retained. Exact occurrences of the comparison payload divided by this known-observation denominator determine green intensity. A missing ID in a complete, uncapped window lowers coverage; a capped window where the ID is absent is unknown and is excluded.
- A different payload is "similar" only if its DLC matches the comparison payload and the Hamming match ratio across all payload bits is at least 90%. It receives the separate similarity color, not green. Do not compare payloads of different DLCs.
- A missing ID in a complete, uncapped window is shown as absent. A missing ID in a capped window is unknown/not retained. Pending and incomplete windows are labeled and excluded from recurrence denominators.
- Do not infer counters, checksums, signals, ECU identity, or vehicle meaning from these colors.

## UI scope

Add a **Compare snapshots** action to each labeled Events group. Open a resizable in-app dialog over the current LogNook window, with an explicit close button and Escape-key support. Include a short color legend, an ID filter, chronological event columns, a scrollable table, payload and RX-count cells, recurrence percentage, and clear labels for incomplete/omitted data. Use current live/frozen event data so **Freeze ranking** continues to hold the view consistently. Do not open a separate native Tauri window or change acquisition controls.

This slice is live-capture UI state. It adds no new saved file or schema: the existing raw capture and manual marks remain the source evidence. Reconstructing the comparison after closing/reopening a capture is deferred.

## Implementation touchpoints

- `crates/lognook-core/src/can_live_events.rs`: bounded snapshot records, event completion/incomplete handling, deterministic ID ranking.
- `crates/lognook-core/src/lib.rs` and `apps/desktop/src/types.ts`: expose the typed data contract.
- `apps/desktop/src/CanTrafficPanel.svelte` and a focused dialog component if needed: open, filter, render, and close the comparison.
- The existing CAN event tests and desktop UI checks should cover matching, missing/unknown, incomplete, bounded, and dialog behavior before this design is considered implemented.

## Acceptance criteria

1. Two or more completed marks with the same label display a separate column for each event and the latest RX payload/count for each retained ID.
2. Repeated exact payloads are green with coverage-based intensity; payloads at or above the 90% same-ID/same-DLC bit-match threshold use the similar color; lower matches remain neutral.
3. Standard and extended forms of the same numeric ID remain separate; labels never mix.
4. Complete uncapped windows distinguish absent IDs from capped/unknown IDs. Pending and incomplete windows do not inflate recurrence.
5. The 100-ID and existing frame/event limits remain bounded and visible to the operator.
6. Opening/closing the resizable dialog does not pause capture or issue bus traffic; freeze/resume behavior follows the existing event snapshot state.

