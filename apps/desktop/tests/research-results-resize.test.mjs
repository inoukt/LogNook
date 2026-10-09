import assert from "node:assert/strict";
import test from "node:test";

const resize = await import("../src/researchResultsResize.ts").catch(() => null);

test("research result resize applies direction and viewport bounds", () => {
  assert.ok(resize, "research result resize helper is available");
  assert.equal(resize.adjustResearchResultsHeight(190, 20, 1000), 210);
  assert.equal(resize.adjustResearchResultsHeight(190, -20, 1000), 170);
  assert.equal(resize.adjustResearchResultsHeight(110, -1, 1000), 120);
  assert.equal(resize.adjustResearchResultsHeight(400, 0, 1000), 360);
  assert.equal(resize.adjustResearchResultsHeight(250, 1, 500), 200);
  assert.equal(resize.adjustResearchResultsHeight(80, -1, 250), 100);
});
