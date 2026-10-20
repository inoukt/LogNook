import assert from "node:assert/strict";
import test from "node:test";
import { clampSignalPanelWidth, DEFAULT_SIGNAL_PANEL_WIDTH } from "../src/signalPanelLayout.ts";

test("shared panel width rejects invalid preferences and preserves graph space", () => {
  assert.equal(clampSignalPanelWidth(Number.NaN), DEFAULT_SIGNAL_PANEL_WIDTH);
  assert.equal(clampSignalPanelWidth(0), 240);
  assert.equal(clampSignalPanelWidth(900), 640);
  assert.equal(clampSignalPanelWidth(420, 1400), 420);
  assert.equal(clampSignalPanelWidth(640, 900), 538);
  assert.equal(clampSignalPanelWidth(420, 781), 419);
  assert.equal(clampSignalPanelWidth(420, 1400), 420, "a larger window can restore the preferred width");
  assert.equal(clampSignalPanelWidth(500, 300), 240);
});
