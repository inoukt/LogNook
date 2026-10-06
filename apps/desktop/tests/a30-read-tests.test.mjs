import assert from "node:assert/strict";
import test from "node:test";

const modes = await import("../src/a30ReadTests.ts").catch(() => null);

test("A30 one-shot controls cover the captured read modes and Scion Mode 06 TIDs", () => {
  assert.ok(modes, "A30 read-mode list is available");
  const ids = modes.A30_READ_TESTS.map(([id]) => id);
  for (const id of [
    "mode_01_engine_rpm",
    "mode_02_freeze_frame_rpm",
    "mode_03_stored_dtcs",
    "mode_06_bitmap_00",
    "mode_06_bitmap_20",
    "mode_06_bitmap_40",
    "mode_06_bitmap_60",
    "mode_06_bitmap_80",
    "mode_06_bitmap_a0",
    "mode_06_tid_35",
    "mode_06_tid_a2",
    "mode_06_tid_a5",
    "mode_07_pending_dtcs",
    "mode_09_vin",
    "mode_0a_permanent_dtcs",
  ]) assert.ok(ids.includes(id), `missing captured read ${id}`);
  assert.equal(ids.some((id) => id.includes("mode_04") || id.includes("mode_30")), false);
});
