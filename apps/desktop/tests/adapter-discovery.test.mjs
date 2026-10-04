import assert from "node:assert/strict";
import test from "node:test";

const discovery = await import("../src/adapterDiscovery.ts").catch(() => null);

test("SX port refresh selects a newly detected device and preserves a valid selection", () => {
  assert.ok(discovery, "adapter discovery helper is available");
  assert.deepEqual(discovery.reconcileSxPorts(["COM5", "COM3"], ""), {
    ports: ["COM3", "COM5"],
    selectedPort: "COM3",
    selectionChanged: true,
  });
  assert.deepEqual(discovery.reconcileSxPorts(["COM3", "COM5"], "COM5"), {
    ports: ["COM3", "COM5"],
    selectedPort: "COM5",
    selectionChanged: false,
  });
});

test("SX port refresh clears a removed selection or chooses another remaining port", () => {
  assert.ok(discovery, "adapter discovery helper is available");
  assert.deepEqual(discovery.reconcileSxPorts([], "COM4"), {
    ports: [],
    selectedPort: "",
    selectionChanged: true,
  });
  assert.deepEqual(discovery.reconcileSxPorts(["COM7"], "COM4"), {
    ports: ["COM7"],
    selectedPort: "COM7",
    selectionChanged: true,
  });
});
