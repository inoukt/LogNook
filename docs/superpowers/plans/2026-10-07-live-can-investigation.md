# Live CAN investigation — general plan

Status: implemented 2026-10-07; real Candlelight/RH-02 acceptance remains pending.

Order: feature 2 (live event correlation), then feature 3 (visual CAN signal editor).

## 2. Live CAN event correlation

### Operator flow

1. Connect the CAN adapter and start a paired capture.
2. Enter an action label and mark each physical action, such as toggling a control six times in 30 seconds. Reuse the existing before/after window controls.
3. After each post-event window finishes, update a compact ranked candidate list in the right-hand traffic panel.
4. Select a candidate to pin its ID and inspect changed bytes/bits, before/after values, recurrence across trials, and RX rate changes.
5. Freeze the ranking for inspection while capture continues; clear a trial group explicitly when beginning another experiment.

### Implementation slices

- **Complete event windows in Rust.** The existing CAN worker now keeps a bounded pre-event RX ring and independent post-event windows. The session clock completes windows even when the bus goes silent; stop/disconnect reports pending marks as incomplete.
- **Reuse correlation rules.** Live and replay both use `CanEventCorrelator`. Recurrence counts unchanged observed trials, 11-bit and 29-bit IDs remain distinct, and only RX traffic ranks vehicle actions. TX remains in raw capture.
- **Rank explainable changes.** The shortlist shows byte transitions and XOR masks, appearance/disappearance, pre/post RX rates normalized by observed trials' configured windows, and counter/additive-checksum pattern hints. It shows counts and coverage rather than a confidence score; correlation does not establish signal meaning.
- **Publish compact results.** The worker emits bounded snapshots on event marks/window completion, not per frame. Analysis uses the Rust receive stream, independent of the UI's 400-frame buffer.
- **Persist evidence.** Manual event records and timestamps remain in the existing JSONL sidecar. Clear trials only resets live analysis; the raw capture and marks remain available for replay.
- **Present the shortlist.** The Events view beside Counts/Frames in `CanTrafficPanel.svelte` has label groups, completed/pending/incomplete counts, ID filtering, pin/freeze, clear, byte transitions, and candidate-to-editor handoff.

Implemented limits: 50,000 history frames, 10,000 frames per mark, 32 pending windows, 256 marks, 64 labels, 1,024 candidate IDs per label, and 100 visible ranked IDs per label. Clearing live results also cancels pending analysis but leaves raw CAN CSV/JSONL evidence and its event marks intact. Candidate rows keep an exact RX frame for editor handoff. The bounded shortlist reports changed/observed recurrence, actual configured pre/post window rates, XOR masks, and counter/additive-checksum pattern hints; these remain correlation hints, not decoders.

### Acceptance

- A repeated synthetic switch transition ranks above unrelated traffic; unchanged trials lower recurrence.
- Standard/extended IDs, unequal windows, overlapping marks, missing frames, and noisy counters remain distinguishable.
- A silent bus completes the timer with no data; disconnect reports an incomplete trial.
- Live and replay results agree for the same complete capture windows.
- Fake transport verifies worker event delivery, capture evidence retention, and a 128-frame burst under the existing 500 ms no-per-frame-delay bound; this is not a real-bus throughput benchmark.

## 3. Visual CAN signal editor

### Operator flow

1. Open a pinned traffic ID or a candidate from Events.
2. See the latest RX payload as eight byte cells with a selectable bit grid. Show the ID width, latest DLC, timestamp, and stale state.
3. Select a byte/bit range, then choose Intel/Motorola byte order, signedness, factor, offset, unit, and name. Provide byte-aligned 8/16/32-bit shortcuts.
4. Preview the selected raw integer and scaled value against the received frame. Display unsupported/short-frame states explicitly.
5. Save the field and add it to the existing graph picker. Edit or remove it through the same editor.

### Implementation slices

- **Expose an editor entry point.** Traffic counts, recent RX frames, and event candidates open the compact right-workspace popout; the adapter sidebar remains unchanged.
- **Render exact selection.** The editor uses the current `CanSignalDefinition` contract, labels LSB0 numbering, highlights the actual Intel/Motorola extraction path across bytes, and supports mouse and keyboard selection.
- **Preview through Rust.** The editor coalesces preview requests, freezes the selected RX frame, and calls the existing `decode_can_signal` implementation. Preview does not publish a measurement.
- **Apply through the current pipeline.** Save/update/remove uses `configure_can_signals`, retains existing IDs while editing, clears stale samples when definitions change, and persists fields/graph selection. Decoded RX samples enter the ordinary graph/record/CSV/replay pipeline.
- **Keep configuration attributable.** Retain exact CAN ID and frame width, extraction, scaling, and unit. The current `CanSignalDefinition` has no persisted note field, so adding annotations is deferred until the shared model supports them. A manually authored field remains an operator definition; avoid inferring vehicle validation or automatically applying it to an unrelated vehicle profile.

### Acceptance

- Grid selection and Rust preview agree for Intel/Motorola, signed values, cross-byte fields, and 64-bit limits.
- Variable DLC, invalid ranges/scaling, missing RX data, and stale frames do not produce fabricated values.
- Save/edit/remove, graph selection, restart restoration, and recording/CSV/replay preserve the existing measurement contract.
- The popout stays usable in a small window; dragging/selecting bits does not interrupt graph rendering.

The editor opens from count rows, recent RX frames, or event candidates; it freezes the selected frame, supports keyboard-accessible bit selection and Intel/Motorola 8/16/32-bit shortcuts, previews with `decode_can_signal`, and saves/edits/removes definitions through the existing Workbench command and graph-selection persistence.

### Repeated-event snapshots

For repeated physical actions, each mark uses the configured pre/post window. The live `CanLiveEventAnalysis` payload includes ordered, capture-local per-mark summaries: sequence and mark time, configured window durations, pending/complete/incomplete state, omitted-ID count, and at most 100 retained RX IDs with frame count and latest exact frame. The existing bounds remain 256 marks, 32 pending windows, and 10,000 frames per window. A window exceeding its frame limit or losing required pre-window history is incomplete; an empty completed window remains comparable. Snapshots are live analysis state only. Clearing live analysis clears these summaries but leaves raw capture data and event marks intact; reopening a saved capture does not reconstruct the comparison.

The Events view can open **Compare snapshots** for one label. Standard and extended IDs remain separate. Green means exact payload recurrence among known completed observations, with intensity reflecting recurrence; amber means a different payload for the same CAN ID and standard/extended width has the same DLC and at least 90% matching bits against the most common exact payload for that same ID and width. Neutral means an observed payload does not meet that similarity threshold. Absent means the ID was not seen in a complete uncapped window; unknown/not retained means the window's ID cap prevents that conclusion. Pending and incomplete windows are labeled and excluded from recurrence. At least two known observations are required before relation colors are applied. These colors describe byte-pattern correlation only: they do not decode signals or establish ECU, counter, checksum, or vehicle meaning.

## Delivery scope

The implementation reuses the existing CAN owner, capture/evidence, correlation, field decoder, and workspace graph APIs. It adds a focused editor component rather than enlarging `CanWorkbenchPanel.svelte`. No adapter firmware change is needed for either feature.

Deferred work is listed in [the feature backlog](../../FEATURE_BACKLOG.md).
