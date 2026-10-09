import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const share = await readFile(new URL("../src/SessionShare.svelte", import.meta.url), "utf8");
const app = await readFile(new URL("../src/App.svelte", import.meta.url), "utf8");
const registration = await readFile(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");

test("session share shows upload disclosure, limits and local CSV alternative", () => {
  assert.match(share, /Anyone with the link/);
  assert.match(share, /200 MB/);
  assert.match(share, /No scheduled expiry/);
  assert.match(share, /Upload public link/);
  assert.match(share, /Save CSV locally/);
  assert.match(share, /onclick=\{upload\}/);
  assert.match(share, /navigator.clipboard.writeText\(link\)/);
  assert.match(registration, /commands::share_recording_link/);
});

test("rec from connect uses backend retained data without replacing ordinary Record", () => {
  assert.match(app, /MultiAdaptCan-v1/);
  assert.match(app, /onclick=\{startRecording\}/);
  assert.match(app, /onclick=\{recordFromConnect\}/);
  assert.match(app, /invoke<[^\n]+>\("start_recording_from_connect"\)/);
  assert.match(app, /older samples unavailable/);
  assert.match(registration, /commands::start_recording_from_connect/);
});
