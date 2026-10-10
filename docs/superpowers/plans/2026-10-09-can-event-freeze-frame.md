# CAN Event Freeze-Frame Comparison Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add bounded per-mark CAN payload snapshots and a resizable in-app comparison dialog that highlights exact and bit-similar recurring messages.

**Architecture:** The Rust live event analyzer will retain at most 100 latest-frame/count summaries per completed event window, keeping raw CAN capture as the complete evidence. The typed event snapshot will carry those summaries to a focused Svelte dialog, which compares same-label, same-width CAN IDs and applies exact recurrence and 90% bit-match colors.

**Tech Stack:** Rust + Serde in `lognook-core`; Svelte 5 + TypeScript; Node UI tests; existing Tauri `CanLiveEventAnalysis` event stream.

**Spec:** `docs/superpowers/specs/2026-10-09-can-event-freeze-frame-design.md`

## Global Constraints

- Keep at most 100 distinct CAN IDs per completed mark window.
- Respect 10,000 frames per event window, 32 pending windows, and 256 marks per analysis.
- Compare only RX messages within one event label and one standard/extended CAN ID form.
- Similarity requires the same DLC and at least 90% matching payload bits.
- Do not add bus traffic, signal decoding, or a new saved artifact/schema.
- Preserve the existing uncommitted `CanTrafficPanel.svelte` and `CanSignalEditor.svelte` changes.

## Review Focus

1. **Capped ID list:** a missing ID in a window with omitted IDs is unknown, not absent; pin this in the UI comparison helper tests.
2. **Incomplete/pending windows:** these carry no comparable payload and do not affect recurrence; pin this in Rust analyzer and helper tests.
3. **Similarity boundary and DLC:** exactly 90% with equal DLC is similar; below 90% or unequal DLC is different; pin the boundary in helper tests.
4. **Mode ties and repeated IDs:** choose the earliest observed modal payload deterministically, and keep 11-bit/29-bit forms distinct; pin this in helper tests.
5. **Live updates/frozen ranking:** the dialog follows the same event snapshot selected by the existing Freeze ranking control and closes if the capture/group disappears; pin the component contract in UI tests.

## File Map

- Modify `crates/lognook-core/src/can_live_events.rs`: keep ordered mark-window states and bounded per-ID summaries.
- Modify `crates/lognook-core/src/lib.rs`: export the new event-window/message types.
- Modify `apps/desktop/src/types.ts`: mirror the serialized Rust event contract.
- Create `apps/desktop/src/canEventSnapshots.ts`: derive modal payload mode, recurrence, similarity, and cell states from a group snapshot.
- Create `apps/desktop/src/CanEventSnapshotsDialog.svelte`: render the accessible, resizable comparison dialog.
- Modify `apps/desktop/src/CanTrafficPanel.svelte`: add the per-label dialog action and use its current live/frozen group.
- Modify `crates/lognook-core/tests/live_can_events.rs` and create `apps/desktop/tests/can-event-snapshots.test.mjs`: cover bounded capture and comparison rules.
- Update `docs/superpowers/plans/2026-10-07-live-can-investigation.md`, `docs/DISCOVERY_DIAGNOSTICS.md`, and `PROJECT_STATUS.md` after implementation.

---

### Task 1: Publish bounded per-mark snapshots from Rust

**Files:**
- Modify: `crates/lognook-core/src/can_live_events.rs`
- Modify: `crates/lognook-core/src/lib.rs`
- Test: `crates/lognook-core/tests/live_can_events.rs`

**Interfaces:**
- Add `CanLiveEventWindowState::{Pending, Complete, Incomplete}` serialized in snake case.
- Add `CanLiveEventMessageSnapshot { can_id: u32, extended: bool, frame_count: u64, latest_frame: CanFrame }`.
- Add `CanLiveEventWindowSnapshot { sequence: u64, elapsed_ns: i64, pre_ns: i64, post_ns: i64, state: CanLiveEventWindowState, omitted_ids: usize, messages: Vec<CanLiveEventMessageSnapshot> }`.
- Add `windows: Vec<CanLiveEventWindowSnapshot>` to `CanLiveEventGroup`.
- Assign a monotonically increasing capture-local sequence when accepting a mark. Preserve it across `clear_results`; clearing removes the windows and ranking but not receive history or sequence.

- [ ] **Step 1: Add Rust regression tests** for a pending mark becoming a completed snapshot, latest RX payload and count per ID, standard/extended separation, and a completed window with no frames.
- [ ] **Step 2: Add bound/incomplete tests** proving the 100-ID cap and deterministic count/ID ordering, correct `omitted_ids`, incomplete windows for truncated/lost-history/stopped marks, and sequence continuity across clear.
- [ ] **Step 3: Run the focused Rust tests**

Run: `cargo test -p lognook-core --test live_can_events`
Expected: tests compile; new regression tests fail until implementation, then pass.

- [ ] **Step 4: Implement the public snapshot types and analyzer state** in `can_live_events.rs`; update the group's pending window at mark acceptance, then transition it to complete or incomplete during `advance`/`finish`.
- [ ] **Step 5: Build completed message summaries** from each window's RX frames, keeping only the latest frame/count for each ID, sorting by count descending then width/ID ascending, retaining 100, and recording the omitted count.
- [ ] **Step 6: Export the public types** from `crates/lognook-core/src/lib.rs` and rerun `cargo test -p lognook-core --test live_can_events`.
- [ ] **Step 7: Commit only the Rust core and its tests** with `feat: publish bounded CAN event snapshots`.

### Task 2: Implement deterministic comparison rules in TypeScript

**Files:**
- Modify: `apps/desktop/src/types.ts`
- Create: `apps/desktop/src/canEventSnapshots.ts`
- Test: `apps/desktop/tests/can-event-snapshots.test.mjs`

**Interfaces:**
- Mirror the Rust `CanLiveEventWindowState`, message, and window fields in TypeScript; `CanLiveEventGroup.windows` is required.
- Export `compareCanEventSnapshots(group: CanLiveEventGroup): CanEventSnapshotRow[]`.
- Each row contains the CAN ID/width, known-observation count, exact recurrence percent, and ordered cells. Each cell state is `pending`, `incomplete`, `absent`, `unknown`, or `observed`; observed cells include frame count, payload bytes, relation (`exact`, `similar`, or `different`), and optional bit-match percent.
- For a row, compare the most frequent exact payload across complete, known observations; break ties by earliest event sequence. Use integer bit counts to apply the `>=90%` threshold without rounding.

- [ ] **Step 1: Add UI helper tests** for exact recurrence and green coverage, a 90% boundary, unequal DLC, a mode tie, absent versus omitted/unknown, incomplete/pending denominator exclusion, and 11-bit/29-bit ID separation.
- [ ] **Step 2: Run `npm run test:ui` in `apps/desktop`** and confirm the new tests fail for the missing helper.
- [ ] **Step 3: Implement `compareCanEventSnapshots`** with the spec's known-observation denominator, earliest-event mode tie-break, and exact/similar/different classifications.
- [ ] **Step 4: Rerun `npm run test:ui`** and confirm the new helper tests pass.
- [ ] **Step 5: Commit only the types/helper/tests** with `feat: compare CAN event payload snapshots`.

### Task 3: Add the in-app comparison dialog

**Files:**
- Create: `apps/desktop/src/CanEventSnapshotsDialog.svelte`
- Modify: `apps/desktop/src/CanTrafficPanel.svelte`
- Test: `apps/desktop/tests/can-event-snapshots.test.mjs`

**Interfaces:**
- Dialog props: `group: CanLiveEventGroup` and `onClose: () => void`.
- Per group, `CanTrafficPanel` stores the selected label, resolves that label from the existing `eventSnapshot` (`frozenEvents ?? liveEventAnalysis`), and passes the resolved group to the dialog.
- If the active capture changes or the selected label disappears, close the dialog. Otherwise the dialog receives live updates or remains frozen through the same existing `eventSnapshot` choice.

- [ ] **Step 1: Add component contract assertions** for the Compare snapshots action, dialog role/accessible name, close button, Escape handling, legend, and use of the comparison helper.
- [ ] **Step 2: Add the group action and dialog mounting** in `CanTrafficPanel.svelte`, preserving its existing filtering, ranking, pinning, and the two existing local CAN UI edits.
- [ ] **Step 3: Render the table** in `CanEventSnapshotsDialog.svelte`: chronological mark columns, CAN ID rows, payload/count cells, recurrence percent, exact/similar/different colors, and explicit absent/unknown/pending/incomplete states.
- [ ] **Step 4: Bound rendered rows to 100 after applying the ID filter**; sort by exact recurrence descending, total retained frames descending, then ID. Show matching-row totals so the operator can narrow the filter when there are more rows.
- [ ] **Step 5: Make the dialog resizable and independently scrollable**, label its color legend, support Close and Escape without changing capture state, and keep it inside the existing LogNook window.
- [ ] **Step 6: Run the focused UI tests and `npm run build`** in `apps/desktop`.
- [ ] **Step 7: Commit only the dialog, panel, and UI tests** with `feat: add CAN event snapshot comparison dialog`.

### Task 4: Update operator/project documentation and verify integration

**Files:**
- Modify: `docs/superpowers/plans/2026-10-07-live-can-investigation.md`
- Modify: `docs/DISCOVERY_DIAGNOSTICS.md`
- Modify: `PROJECT_STATUS.md`

- [ ] **Step 1: Update the CAN investigation notes** with the per-event data contract, limits, color interpretation, and live-only persistence boundary.
- [ ] **Step 2: Update the CAN operator guide** to explain marking repeated actions, opening Compare snapshots, reading exact/similar/absent/unknown colors, and that correlation does not decode meaning.
- [ ] **Step 3: Add a dated project-status entry** that states software verification results and clearly notes that no real CAN hardware acceptance was performed.
- [ ] **Step 4: Run the targeted Rust tests, `npm run test:ui`, `npm run build`, and `git diff --check`**; review outputs and confirm existing CAN UI changes remain present.
- [ ] **Step 5: Commit only the documentation files** with `docs: document CAN event snapshot comparison`.

## Handoff Notes

The feature changes the live `CanLiveEventAnalysis` payload contract and renders a comparison derived from that payload. It does not change raw CAN capture files, send adapter traffic, or reconstruct snapshots after a saved capture is reopened. Review each task's diff before proceeding; never stage or commit the already-present `CanTrafficPanel.svelte` and `CanSignalEditor.svelte` edits unless they were intentionally changed by that task.

