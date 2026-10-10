import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const snapshots = await import("../src/canEventSnapshots.ts").catch(() => null);
const dialogSource = await readFile(new URL("../src/CanEventSnapshotsDialog.svelte", import.meta.url), "utf8");
const panelSource = await readFile(new URL("../src/CanTrafficPanel.svelte", import.meta.url), "utf8");

function window(sequence, state, entries = [], omitted_ids = 0) {
  return {
    sequence, elapsed_ns: sequence, pre_ns: 0, post_ns: 0, state, omitted_ids,
    messages: entries.map(([can_id, extended, data, frame_count = 1]) => ({
      can_id, extended, frame_count,
      latest_frame: { elapsed_ns: sequence, wall_time_unix_ns: sequence, can_id, extended, direction: "rx", data, flags: 0 },
    })),
  };
}

function group(windows) {
  return { label: "switch", completed_events: 0, pending_events: 0, incomplete_events: 0, omitted_candidates: 0, candidates: [], windows };
}

function rowsFor(windows) {
  assert.equal(typeof snapshots?.compareCanEventSnapshots, "function", "comparison helper is exported");
  return snapshots.compareCanEventSnapshots(group(windows));
}

test("exact recurrence uses known complete observations as its denominator", () => {
  const rows = rowsFor([
    window(1, "complete", [[291, false, [0x12, 0x34], 4]]),
    window(2, "complete", [[291, false, [0x12, 0x34], 7]]),
    window(3, "complete", [[291, false, [0x12, 0x35]]]),
    window(4, "complete", []),
  ]);
  const row = rows[0];
  assert.equal(row.known_observations, 4);
  assert.equal(row.exact_recurrence_percent, 50);
  assert.deepEqual(row.cells.map(({ state }) => state), ["observed", "observed", "observed", "absent"]);
  assert.deepEqual(row.cells.slice(0, 2).map(({ relation }) => relation), ["exact", "exact"]);
  assert.equal(row.cells[0].frame_count, 4);
  assert.deepEqual(row.cells[0].payload, [0x12, 0x34]);
});

test("same-DLC payload at exactly 90 percent matching bits is similar", () => {
  const rows = rowsFor([
    window(1, "complete", [[42, false, [0, 0, 0, 0, 0]]]),
    window(2, "complete", [[42, false, [0, 0, 0, 0, 15]]]),
  ]);
  assert.equal(rows[0].cells[1].relation, "similar");
  assert.equal(rows[0].cells[1].bit_match_percent, 90);
});

test("unequal DLC is different even when overlapping bits match", () => {
  const rows = rowsFor([
    window(1, "complete", [[42, false, [0]]]),
    window(2, "complete", [[42, false, [0, 0]]]),
  ]);
  assert.equal(rows[0].cells[1].relation, "different");
  assert.equal(rows[0].cells[1].bit_match_percent, undefined);
});

test("mode ties use the payload from the earliest event sequence", () => {
  const rows = rowsFor([
    window(8, "complete", [[42, false, [1, 0, 0, 0, 0]]]),
    window(3, "complete", [[42, false, [0, 0, 0, 0, 0]]]),
  ]);
  assert.equal(rows[0].cells[1].relation, "similar");
  assert.equal(rows[0].cells[0].relation, "exact");
});

test("complete uncapped absence differs from capped unknown", () => {
  const rows = rowsFor([
    window(1, "complete", [[42, false, [1]]]),
    window(2, "complete", [], 1),
    window(3, "complete", []),
  ]);
  assert.equal(rows[0].cells[1].state, "unknown");
  assert.equal(rows[0].cells[2].state, "absent");
  assert.equal(rows[0].known_observations, 2);
  assert.equal(rows[0].exact_recurrence_percent, 50);
});

test("pending and incomplete windows are shown but excluded from recurrence", () => {
  const rows = rowsFor([
    window(1, "complete", [[42, false, [1]]]),
    window(2, "incomplete", [[42, false, [2]]]),
    window(3, "pending"),
  ]);
  assert.deepEqual(rows[0].cells.map(({ state }) => state), ["observed", "incomplete", "pending"]);
  assert.equal(rows[0].known_observations, 1);
  assert.equal(rows[0].exact_recurrence_percent, 100);
});

test("standard and extended forms of the same numeric CAN ID stay separate", () => {
  const rows = rowsFor([
    window(1, "complete", [[291, false, [1]], [291, true, [2]]]),
    window(2, "complete", [[291, false, [1]], [291, true, [2]]]),
  ]);
  assert.deepEqual(rows.map(({ can_id, extended }) => [can_id, extended]), [[291, false], [291, true]]);
});

test("snapshot comparison component exposes the accessible dialog contract", () => {
  assert.match(panelSource, /Compare snapshots/);
  assert.match(panelSource, /<CanEventSnapshotsDialog\s+group=\{selectedSnapshotGroup\}/);
  assert.match(dialogSource, /<dialog[\s\S]*aria-labelledby="can-snapshot-title"/);
  assert.match(dialogSource, /aria-label="Close snapshot comparison"/);
  assert.match(dialogSource, /oncancel=\{\(event\) => \{ event\.preventDefault\(\); close\(\); \}\}/);
  assert.match(dialogSource, /aria-label="Snapshot comparison color legend"/);
  assert.match(dialogSource, /compareCanEventSnapshots\(group\)/);
});

test("one known observation keeps payload visible but gates relation color and recurrence", () => {
  const rows = rowsFor([
    window(1, "complete", [[42, false, [0x2a], 3]]),
    window(2, "pending"),
    window(3, "incomplete"),
    window(4, "complete", [], 1),
  ]);
  assert.equal(rows[0].known_observations, 1);
  assert.equal(rows[0].cells[0].state, "observed");
  assert.deepEqual(rows[0].cells[0].payload, [0x2a]);
  assert.equal(rows[0].cells[0].frame_count, 3);
  assert.deepEqual(rows[0].cells.slice(1).map(({ state }) => state), ["pending", "incomplete", "unknown"]);
  assert.match(dialogSource, /row\.known_observations >= 2 \? `cell-\$\{cell\.relation\}` : "cell-observed-uncertain"/);
  assert.match(dialogSource, /Needs more observations/);
  assert.match(dialogSource, /relation needs more observations/);
  assert.match(dialogSource, /knownObservations >= 2 \? exactRecurrencePercent : 0/);
  assert.match(dialogSource, /Payload relation needs at least two known observations/);
});

test("selected comparison invalidates when the live capture disappears or changes", () => {
  assert.match(panelSource, /selectedSnapshotLabel && \(!liveEventAnalysis/);
  assert.match(panelSource, /liveEventAnalysis\.capture_id !== selectedSnapshotCaptureId/);
  assert.match(panelSource, /!eventSnapshot\.groups\.some\(\(group\) => group\.label === selectedSnapshotLabel\)/);
  assert.match(panelSource, /disabled=\{!liveEventAnalysis \|\| liveEventAnalysis\.capture_id !== eventSnapshot\?\.capture_id\}/);
});
