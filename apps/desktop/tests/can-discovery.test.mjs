import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import ts from "typescript";

const canDiscoverySource = await readFile(new URL("../src/canDiscovery.ts", import.meta.url), "utf8");
const canWorkbenchUrl = new URL("../src/canWorkbench.ts", import.meta.url).href;
const compiledCanDiscovery = ts.transpileModule(
  canDiscoverySource.replace(/from "\.\/canWorkbench(?:\.ts)?"/, `from "${canWorkbenchUrl}"`),
  { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } },
).outputText;
const canDiscovery = await import(`data:text/javascript,${encodeURIComponent(
  compiledCanDiscovery,
)}`);
const { discoverySeed, changedBits, canScopePoints, scopePath } = canDiscovery;
const canWorkbench = await import("../src/canWorkbench.ts");
const traffic = await readFile(new URL("../src/CanTrafficPanel.svelte", import.meta.url), "utf8");
const editor = await readFile(new URL("../src/CanSignalEditor.svelte", import.meta.url), "utf8");
const editorScript = editor.match(/<script lang="ts">([\s\S]*?)<\/script>/)?.[1];

function makeEditorHarness(initialFrame, invoke) {
  assert.ok(editorScript, "CAN signal editor has a script");
  const timers = new Map();
  let nextTimer = 0;
  const setTimeout = (callback) => {
    const id = ++nextTimer;
    timers.set(id, callback);
    return id;
  };
  const clearTimeout = (id) => timers.delete(id);
  const fireTimer = () => {
    const [id, callback] = timers.entries().next().value ?? [];
    assert.ok(callback, "a preview timer is queued");
    timers.delete(id);
    return callback();
  };
  const script = ts.transpileModule(
    editorScript
      .replace(/^\s*import .*?;\s*$/gm, "")
      .replace(/^\s*export /gm, "")
      .replace("let frame: CanFrame;", "let frame: CanFrame = initialFrame;"),
    { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None } },
  ).outputText;
  const runnable = `let liveFrame, scope, matchingFields, selectedBits, rawValue, previewRawValue, frameAgeNs, definitionError;\n${script}`;
  const create = new Function(
    "initialFrame", "invoke", "onMount", "onDestroy", "setTimeout", "clearTimeout", "fireTimer",
    "canSignalBitPositions", "canSignalDefinitionError", "extractCanSignalRaw", "canScopePoints",
    `${runnable}
      return {
        schedulePreview,
        setFrame(next) { frame = next; latestElapsedNs = next.elapsed_ns; refreshFramePreview(frame); },
        fireTimer,
        snapshot: () => ({
          preview, previewError, previewSequence, previewFrameElapsed: previewFrame?.elapsed_ns ?? null,
          previewRaw: preview && previewFrame
            ? extractCanSignalRaw(previewFrame.data, startBit, bitLength, byteOrder, signed)?.toString() ?? null
            : rawValue?.toString() ?? null,
        }),
      };
    `,
  );
  return create(
    initialFrame, invoke, () => {}, () => {}, setTimeout, clearTimeout, fireTimer,
    canWorkbench.canSignalBitPositions, canWorkbench.canSignalDefinitionError,
    canWorkbench.extractCanSignalRaw, canScopePoints,
  );
}

const candidate = { changed_bytes: [
  { byte_index: 0, changed_events: 1, transitions: [{ before: null, after: 3 }] },
  { byte_index: 3, changed_events: 6, transitions: [{ before: 0, after: 4 }, { before: 4, after: 5 }] },
] };
test("event discovery seeds the recurring byte or the explicitly selected bit", () => {
  assert.deepEqual(discoverySeed(candidate, "Switch"), { startBit: 24, bitLength: 8, name: "Switch · B3" });
  assert.deepEqual(discoverySeed(candidate, "Switch", 3, 2), { startBit: 26, bitLength: 1, name: "Switch · B3.2" });
  assert.deepEqual(changedBits(candidate, 3), [0, 2]);
  assert.deepEqual(changedBits(candidate, 0), []);
});
test("scope keeps matching RX samples only and handles constant signals", () => {
  const frame = { direction: "rx", can_id: 123, extended: false, wall_time_unix_ns: 100, data: [4] };
  const points = canScopePoints([frame, { ...frame, direction: "tx" }, { ...frame, extended: true }, { ...frame, wall_time_unix_ns: 200 }], 123, false, 2, 1, "intel", false, 2, -1);
  assert.deepEqual(points, [{ time: 100, value: 1 }, { time: 200, value: 1 }]);
  assert.equal(scopePath(points), "M0.00,25.00 L100.00,25.00");
  assert.equal(scopePath([]), "");
  assert.deepEqual(canScopePoints([frame], 123, false, 8, 16, "intel", false, 1, 0), []);
});

test("scope stays bounded to the newest 120 matching frames", () => {
  const frames = Array.from({ length: 121 }, (_, index) => ({
    elapsed_ns: index,
    wall_time_unix_ns: index,
    can_id: 123,
    extended: false,
    direction: "rx",
    data: [index],
    flags: 0,
  }));
  const points = canScopePoints(frames, 123, false, 0, 8, "intel", false, 1, 0);
  assert.equal(points.length, 120);
  assert.deepEqual(points[0], { time: 1, value: 1 });
  assert.deepEqual(points.at(-1), { time: 120, value: 120 });
});

test("event candidates wire changed fields and evidence into the shared live editor", () => {
  assert.match(traffic, /changedBits\(item\.candidate, byte\.byte_index\)/);
  assert.match(traffic, /discoverySeed\(candidate, label, byte, bit\)/);
  assert.match(traffic, /initialStartBit=\{editor\.seed\?\.startBit \?\? 0\}/);
  assert.match(traffic, /initialBitLength=\{editor\.seed\?\.bitLength \?\? 16\}/);
  assert.match(traffic, /initialName=\{editor\.seed\?\.name \?\? ""\}/);
  assert.match(traffic, /evidence=\{editor\.evidence \?\? ""\}/);
  assert.match(traffic, /eventBytes\(candidate\)/);
  assert.match(traffic, /history=\{frames\}/);
  assert.match(editor, /preview_can_signal/);
  assert.match(editor, /canScopePoints\(history/);
  assert.match(editor, /Follow live RX/);
  assert.match(editor, /frame = followLive && liveFrame \? liveFrame : evidenceFrame/);
  assert.match(editor, /previewTimer = undefined/);
  assert.match(editor, /if \(previewInFlight\) previewPending = true/);
  assert.match(editor, /else if \(!previewTimer\) schedulePreviewTask\(false\)/);
  assert.match(editor, /\{followLive \? "Live RX" : "Frozen RX"\}/);
});

test("live preview coalesces frames behind one delayed Rust invoke", async () => {
  const frame = (elapsed_ns, value) => ({
    elapsed_ns, wall_time_unix_ns: elapsed_ns, can_id: 0x123, extended: false,
    direction: "rx", data: [value, 0, 0, 0, 0, 0, 0, 0], flags: 0,
  });
  const firstFrame = frame(1, 1);
  let active = 0;
  let maxActive = 0;
  const calls = [];
  const resolveCalls = [];
  const invoke = (command, args) => {
    calls.push({ command, ...args });
    active++;
    maxActive = Math.max(maxActive, active);
    return new Promise((resolve) => resolveCalls.push((sample) => {
      active--;
      resolve(sample);
    }));
  };
  const harness = makeEditorHarness(firstFrame, invoke);
  harness.schedulePreview();
  const firstTask = harness.fireTimer();
  const latestFrame = frame(5, 5);
  for (let value = 2; value <= 5; value++) harness.setFrame(frame(value, value));
  const callsWhileDelayed = calls.length;
  resolveCalls[0]({
    elapsed_ns: 1, wall_time_unix_ns: 1, source_id: "can:preview", ecu_id: null,
    signal_id: "can:123", value: 10, unit: "", quality: "valid", origin: "live", raw_bytes: [],
  });
  await firstTask;
  assert.equal(callsWhileDelayed, 1, "frame updates must not start overlapping invokes");
  assert.equal(maxActive, 1);
  assert.equal(harness.snapshot().preview?.value, 10, `a live-frame update must not invalidate the in-flight reply: ${JSON.stringify(harness.snapshot())}`);
  assert.equal(harness.snapshot().previewFrameElapsed, firstFrame.elapsed_ns);
  assert.equal(harness.snapshot().previewRaw, "1", "the raw preview stays paired with its decoded frame");

  const latestTask = harness.fireTimer();
  assert.equal(calls.length, 2);
  assert.deepEqual(calls[1].frame, latestFrame);
  resolveCalls[1]({
    elapsed_ns: 5, wall_time_unix_ns: 5, source_id: "can:preview", ecu_id: null,
    signal_id: "can:123", value: 20, unit: "", quality: "valid", origin: "live", raw_bytes: [],
  });
  await latestTask;
  assert.equal(harness.snapshot().preview?.value, 20);
});
