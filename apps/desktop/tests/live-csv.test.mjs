import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const liveCsv = await import("../src/liveCsv.ts").catch(() => null);
const app = await readFile(new URL("../src/App.svelte", import.meta.url), "utf8");

test("live CSV snapshot contains only selected live graph samples in timestamp order", () => {
  assert.equal(typeof liveCsv?.liveCsvSamples, "function");
  if (!liveCsv?.liveCsvSamples) return;

  const rpm = [
    { elapsed_ns: 30, signal_id: "rpm", origin: "live", value: 3 },
    { elapsed_ns: 10, signal_id: "rpm", origin: "live", value: 1 },
  ];
  const coolant = [{ elapsed_ns: 20, signal_id: "coolant", origin: "live", value: 2 }];
  const replay = [{ elapsed_ns: 40, signal_id: "rpm", origin: "replay", value: 4 }];
  const buffer = [...rpm, ...replay, ...coolant, { elapsed_ns: 5, signal_id: "hidden", origin: "live" }];

  const samples = liveCsv.liveCsvSamples(buffer, ["rpm", "coolant", "rpm"]);

  assert.deepEqual(samples.map(({ elapsed_ns, signal_id }) => [elapsed_ns, signal_id]), [
    [10, "rpm"], [20, "coolant"], [30, "rpm"],
  ]);
  assert.equal(buffer.length, 5, "snapshot leaves the source buffer unchanged");
});

test("live CSV export includes graph-only signals rather than only logged PIDs", () => {
  assert.match(app, /liveCsvSamples\(liveCsvBuffer\.snapshot\(\), visibleLiveSignalIds\)/);
  assert.match(app, /hasSignals\(visibleLiveSignalIds\)/);
  assert.match(app, /visibleLiveSignalIds = visibleSignals/);
  assert.match(app, /series\[signal\.id\]\?\.length/);
});

test("live export buffer retains samples independently of the graph buffer and drops oldest at capacity", () => {
  assert.equal(typeof liveCsv?.LiveCsvBuffer, "function");
  if (!liveCsv?.LiveCsvBuffer) return;

  const buffer = new liveCsv.LiveCsvBuffer(3);
  for (const [elapsed_ns, signal_id] of [[1, "rpm"], [2, "coolant"], [3, "rpm"], [4, "coolant"]]) {
    buffer.append({ elapsed_ns, signal_id, origin: "live", value: elapsed_ns });
  }
  buffer.append({ elapsed_ns: 5, signal_id: "rpm", origin: "replay", value: 5 });

  assert.deepEqual(liveCsv.liveCsvSamples(buffer.snapshot(), ["rpm", "coolant"])
    .map(({ elapsed_ns, signal_id }) => [elapsed_ns, signal_id]), [
      [2, "coolant"], [3, "rpm"], [4, "coolant"],
    ]);
  assert.equal(buffer.droppedSamples, 1);

  const fullHistory = new liveCsv.LiveCsvBuffer();
  for (let elapsed_ns = 0; elapsed_ns < 200; elapsed_ns++) {
    fullHistory.append({ elapsed_ns, signal_id: "rpm", origin: "live", value: elapsed_ns });
  }
  assert.equal(fullHistory.snapshot().length, 200, "export history exceeds the 180-point graph window");
});
