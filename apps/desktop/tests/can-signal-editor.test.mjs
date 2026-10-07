import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";

const can = await import("../src/canWorkbench.ts");
const editor = await readFile(new URL("../src/CanSignalEditor.svelte", import.meta.url), "utf8").catch(() => "");
const traffic = await readFile(new URL("../src/CanTrafficPanel.svelte", import.meta.url), "utf8");
const app = await readFile(new URL("../src/App.svelte", import.meta.url), "utf8");

test("visual signal selection follows Intel and Motorola bit numbering across bytes", () => {
  assert.deepEqual(can.canSignalBitPositions(0, 8, "intel"), [0, 1, 2, 3, 4, 5, 6, 7]);
  assert.deepEqual(can.canSignalBitPositions(7, 16, "motorola"), [7, 6, 5, 4, 3, 2, 1, 0, 15, 14, 13, 12, 11, 10, 9, 8]);
  assert.equal(can.canSignalBitPositions(63, 2, "intel"), null);
});

test("raw preview handles endianness, signed values, and payloads shorter than the field", () => {
  assert.equal(can.extractCanSignalRaw([0x34, 0x12], 0, 16, "intel", false), 0x1234n);
  assert.equal(can.extractCanSignalRaw([0x12, 0x34], 7, 16, "motorola", false), 0x1234n);
  assert.equal(can.extractCanSignalRaw([0xff, 0x0f], 0, 12, "intel", true), -1n);
  assert.equal(can.extractCanSignalRaw([0x34], 0, 16, "intel", false), null);
});

test("traffic IDs and event candidates can open the shared signal editor", () => {
  assert.match(traffic, /openTrafficEditor/);
  assert.match(traffic, /sample_frame/);
  assert.match(traffic, /liveEventAnalysis/);
  assert.match(traffic, /Freeze ranking/);
  assert.match(traffic, /before_window_ns/);
  assert.match(traffic, /counter_like_bytes/);
  assert.match(traffic, /changed_events\}\/\{item\.candidate\.observed_events/);
  assert.match(editor, /LSB0 within each byte/);
  assert.match(editor, /preview_can_signal/);
  assert.match(editor, /Save and graph/);
  assert.match(app, /onSaveField=\{saveCanSignalField\}/);
  assert.match(app, /onRemoveField=\{removeCanSignalField\}/);
  assert.match(app, /rememberWorkbenchGraphSignalId\(workbenchGraphSignalIds, field\.id\)/);
});
