import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const browser = await import("../src/sessionBrowser.ts").catch(() => null);

test("recent recordings are sorted newest first without mutating the source list", () => {
  const recordings = [
    { path: "older", name: "older", modified_unix_ms: 100, sample_count: 3 },
    { path: "newer", name: "newer", modified_unix_ms: 300, sample_count: 1 },
    { path: "middle", name: "middle", modified_unix_ms: 200, sample_count: 2 },
  ];

  assert.ok(browser, "session browser helper is available");
  assert.deepEqual(
    browser.recentRecordingsFirst(recordings).map(({ path }) => path),
    ["newer", "middle", "older"],
  );
  assert.deepEqual(recordings.map(({ path }) => path), ["older", "newer", "middle"]);
});

test("recording timestamps fall back to a readable label when the value is invalid", () => {
  assert.ok(browser, "session browser helper is available");
  assert.equal(browser.formatRecordingTime(Number.NaN), "Unknown time");
});

test("opening a session waits until the player has activated it", async () => {
  assert.equal(typeof browser.openAndActivateRecording, "function");
  if (typeof browser.openAndActivateRecording !== "function") return;

  let releaseActivation;
  const activationGate = new Promise((resolve) => { releaseActivation = resolve; });
  let active = false;
  const pending = browser.openAndActivateRecording(
    async () => ({ id: "session" }),
    async () => { await activationGate; active = true; },
  );

  await Promise.resolve();
  assert.equal(active, false);
  releaseActivation();
  await pending;
  assert.equal(active, true);
});

test("session browser exposes inline rename, confirmed delete, and folder actions", async () => {
  const source = await readFile(new URL("../src/SessionBrowser.svelte", import.meta.url), "utf8");
  assert.match(source, /invoke\("rename_recording"/);
  assert.match(source, /invoke\("delete_recording"/);
  assert.match(source, /invoke\("open_recording_folder"/);
  assert.match(source, /Confirm delete/);
  assert.match(source, /New filename for/);
  assert.match(source, /use:focusRenameInput/);
  assert.match(source, /Open folder for/);
});
