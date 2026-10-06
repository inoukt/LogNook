import assert from "node:assert/strict";
import test from "node:test";

const queue = await import("../src/researchQueue.ts").catch(() => null);

const deferred = () => {
  let resolve;
  const promise = new Promise((done) => {
    resolve = done;
  });
  return { promise, resolve };
};

test("selection_survives_search_and_page_changes", () => {
  assert.ok(queue, "research queue helpers are available");

  const pageOne = ["candidate-a", "candidate-b", "candidate-c"];
  const selected = pageOne.reduce(
    (current, id) => queue.toggleCandidateSelection(current, id, id !== "candidate-b"),
    [],
  );
  const afterSearchAndPageChange = queue.toggleCandidateSelection(
    selected,
    "candidate-d",
    true,
  );

  assert.deepEqual(selected, ["candidate-a", "candidate-c"]);
  assert.deepEqual(afterSearchAndPageChange, ["candidate-a", "candidate-c", "candidate-d"]);
  assert.deepEqual(
    queue.toggleCandidateSelection(afterSearchAndPageChange, "candidate-c", true),
    afterSearchAndPageChange,
  );
});

test("queue_awaits_each_item_in_selection_order", async () => {
  assert.ok(queue, "research queue helpers are available");

  const items = ["candidate-z", "candidate-a", "candidate-m"];
  const gates = new Map(items.map((id) => [id, deferred()]));
  const starts = new Map(items.map((id) => [id, deferred()]));
  const started = [];
  const results = [];
  let active = 0;
  let maxActive = 0;

  const run = queue.runResearchQueue(
    items,
    async (id, index) => {
      started.push([id, index]);
      starts.get(id).resolve();
      active += 1;
      maxActive = Math.max(maxActive, active);
      try {
        await gates.get(id).promise;
        return { outcome: index === 1 ? "negativeResponse" : "noResponse" };
      } finally {
        active -= 1;
      }
    },
    () => false,
    (...result) => results.push(result),
  );

  await starts.get(items[0]).promise;
  assert.deepEqual(started, [[items[0], 0]]);
  gates.get(items[0]).resolve();
  await starts.get(items[1]).promise;
  assert.deepEqual(results, [[items[0], 0, { outcome: "noResponse" }]]);
  gates.get(items[1]).resolve();
  await starts.get(items[2]).promise;
  assert.deepEqual(results[1], [items[1], 1, { outcome: "negativeResponse" }]);
  gates.get(items[2]).resolve();
  await run;

  assert.deepEqual(started, items.map((id, index) => [id, index]));
  assert.deepEqual(
    results,
    items.map((id, index) => [id, index, { outcome: index === 1 ? "negativeResponse" : "noResponse" }]),
  );
  assert.equal(maxActive, 1);
});

test("stop_keeps_current_result_and_skips_later_items", async () => {
  assert.ok(queue, "research queue helpers are available");

  const activeRequest = deferred();
  const activeStarted = deferred();
  const started = [];
  const results = [];
  let stopped = false;

  const run = queue.runResearchQueue(
    ["in-flight", "must-skip"],
    (id) => {
      started.push(id);
      activeStarted.resolve();
      return activeRequest.promise;
    },
    () => stopped,
    (...result) => results.push(result),
  );

  await activeStarted.promise;
  stopped = true;
  activeRequest.resolve({ outcome: "completed" });
  await run;

  assert.deepEqual(started, ["in-flight"]);
  assert.deepEqual(results, [["in-flight", 0, { outcome: "completed" }]]);
});

test("transport_error_stops_before_later_items", async () => {
  assert.ok(queue, "research queue helpers are available");

  const transportError = new Error("adapter disconnected");
  const started = [];
  const results = [];

  await queue.runResearchQueue(
    ["disconnects", "must-skip"],
    async (id) => {
      started.push(id);
      throw transportError;
    },
    () => false,
    (...result) => results.push(result),
  );

  assert.deepEqual(started, ["disconnects"]);
  assert.equal(results.length, 1);
  assert.equal(results[0][0], "disconnects");
  assert.equal(results[0][1], 0);
  assert.equal(results[0][2], transportError);
});
