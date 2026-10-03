import assert from "node:assert/strict";
import test from "node:test";

const liveCsv = await import("../src/liveCsv.ts").catch(() => null);

test("live CSV snapshot contains only logged live samples in timestamp order", () => {
  assert.equal(typeof liveCsv?.liveCsvSamples, "function");
  if (!liveCsv?.liveCsvSamples) return;

  const rpm = [
    { elapsed_ns: 30, signal_id: "rpm", origin: "live", value: 3 },
    { elapsed_ns: 10, signal_id: "rpm", origin: "live", value: 1 },
  ];
  const coolant = [{ elapsed_ns: 20, signal_id: "coolant", origin: "live", value: 2 }];
  const replay = [{ elapsed_ns: 40, signal_id: "rpm", origin: "replay", value: 4 }];
  const series = { rpm: [...rpm, ...replay], coolant, hidden: [{ elapsed_ns: 5, signal_id: "hidden", origin: "live" }] };

  const samples = liveCsv.liveCsvSamples(series, ["rpm", "coolant", "rpm"]);

  assert.deepEqual(samples.map(({ elapsed_ns, signal_id }) => [elapsed_ns, signal_id]), [
    [10, "rpm"], [20, "coolant"], [30, "rpm"],
  ]);
  assert.equal(series.rpm.length, 3, "snapshot leaves the source buffer unchanged");
});
