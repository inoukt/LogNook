import assert from "node:assert/strict";
import test from "node:test";
import {
  createSavedCanReplayCommands,
  formatSavedCanId,
  formatSavedCanReplaySpan,
  parseSavedCanReplayRangeMs,
  sameReviewedCanTarget,
} from "../src/savedCanReplay.ts";

test("saved replay time ranges use inclusive integer milliseconds and checked nanoseconds", () => {
  assert.deepEqual(parseSavedCanReplayRangeMs("12", "600000"), {
    start_elapsed_ns: 12_000_000,
    end_elapsed_ns: 600_000_000_000,
  });
  assert.match(parseSavedCanReplayRangeMs("-1", "2").error, /non-negative/);
  assert.match(parseSavedCanReplayRangeMs("1.5", "2").error, /whole milliseconds/);
  assert.match(parseSavedCanReplayRangeMs("3", "2").error, /must not exceed/);
  assert.match(parseSavedCanReplayRangeMs("", "2").error, /whole milliseconds/);
  assert.match(parseSavedCanReplayRangeMs("9007199254741", "9007199254741").error, /too large/);
});

test("standard and extended rows with the same numeric ID remain distinct", () => {
  assert.equal(formatSavedCanId(0x123, false), "11-bit 0x123");
  assert.equal(formatSavedCanId(0x123, true), "29-bit 0x00000123");
});

test("duration labels preserve captured span and paced duration", () => {
  assert.equal(formatSavedCanReplaySpan(0), "0.000 s");
  assert.equal(formatSavedCanReplaySpan(1_234_000_000), "1.234 s");
});

test("saved replay commands use opaque source and preview tokens only", async () => {
  const calls = [];
  const invoke = async (command, args) => {
    calls.push({ command, args });
    return command;
  };
  const commands = createSavedCanReplayCommands(invoke);
  await commands.registerSource("C:/logs/source.candump.csv");
  await commands.prepare("saved-can-source-0001", {
    can_id: 0x123,
    extended: false,
    start_elapsed_ns: 0,
    end_elapsed_ns: 10_000_000,
  });
  await commands.start("saved-can-preview-0001", 7, 9);
  assert.deepEqual(calls, [
    { command: "register_saved_can_replay_source", args: { path: "C:/logs/source.candump.csv" } },
    {
      command: "prepare_saved_can_replay",
      args: {
        sourceId: "saved-can-source-0001",
        selection: { can_id: 0x123, extended: false, start_elapsed_ns: 0, end_elapsed_ns: 10_000_000 },
      },
    },
    {
      command: "start_saved_can_replay",
      args: { previewId: "saved-can-preview-0001", expectedCanGeneration: 7, expectedOutputCaptureId: 9 },
    },
  ]);
});

test("saved CAN Start requires the target and output capture that were reviewed", () => {
  const target = {
    generation: 8, connected: true, source_id: "can:rh02", channel: 1,
    bitrate: 500000, listen_only: false, output_capture_active: true,
    output_capture_id: 21, can_start: true, start_block_reason: null,
  };
  assert.equal(sameReviewedCanTarget(target, { ...target }), true);
  assert.equal(sameReviewedCanTarget(target, { ...target, generation: 9 }), false);
  assert.equal(sameReviewedCanTarget(target, { ...target, channel: 2 }), false);
  assert.equal(sameReviewedCanTarget(target, { ...target, output_capture_id: 22 }), false);
  assert.equal(sameReviewedCanTarget(target, { ...target, output_capture_id: null }), false);
});
