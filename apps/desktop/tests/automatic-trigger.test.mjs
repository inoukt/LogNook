import assert from "node:assert/strict";
import test from "node:test";
import { restoreAutomaticTriggerConfig } from "../src/automaticTrigger.ts";

test("automatic trigger preferences clamp hold and pre-trigger durations and reject invalid values", () => {
  assert.deepEqual(
    restoreAutomaticTriggerConfig(JSON.stringify({
      signal_id: "engine_rpm",
      direction: "below",
      threshold: "not a number",
      hysteresis: -5,
      hold_ms: 900_000,
      pre_trigger_ms: 90_000,
    }), ["engine_rpm", "coolant_temp"], "engine_rpm"),
    {
      signal_id: "engine_rpm",
      direction: "below",
      threshold: 3_000,
      hysteresis: 0,
      hold_ms: 60_000,
      pre_trigger_ms: 30_000,
    },
  );
});

test("automatic trigger preferences fall back to a currently acquired signal", () => {
  assert.equal(
    restoreAutomaticTriggerConfig('{"signal_id":"not_acquired"}', ["coolant_temp"], "coolant_temp").signal_id,
    "coolant_temp",
  );
});
