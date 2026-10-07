import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const settings = await readFile(new URL("../src/SettingsPanel.svelte", import.meta.url), "utf8");
const app = await readFile(new URL("../src/App.svelte", import.meta.url), "utf8");
const commands = await readFile(new URL("../src-tauri/src/commands/agent_access.rs", import.meta.url), "utf8");

test("Agent Access keeps six-digit PIN entry in Settings and exposes explicit lock state", () => {
  assert.match(settings, /type="password" inputmode="numeric" pattern="\[0-9\]\{6\}" maxlength="6"/);
  assert.match(settings, /onAgentLock\(\)/);
  assert.match(settings, /AGENT ACCESS/);
  assert.doesNotMatch(settings, /localStorage\.(?:setItem|getItem)\([^\n]*agentPin/);
  assert.match(app, /invoke<AgentAccessStatus>\("unlock_agent_access"/);
});

test("Agent Access setup command copies the installed executable and never includes a PIN", () => {
  assert.match(commands, /current_exe\(\)/);
  assert.match(commands, /"--agent-mcp"/);
  assert.match(commands, /LOGNOOK_AGENT_SOCKET/);
  assert.doesNotMatch(commands, /"pin"\s*:/);
  assert.match(settings, /Copy agent setup/);
});

test("agent PID selection changes round-trip through workspace state", () => {
  assert.match(app, /agent_sync_signal_selection/);
  assert.match(app, /listen<AgentSignalSelection>\("agent-signal-selection"/);
  assert.match(app, /logSignalIds = payload\.log_signal_ids/);
  assert.match(app, /graphSignalIds = payload\.graph_signal_ids/);
});

test("Agent Access Settings expose active operation and trace gaps", () => {
  assert.match(settings, /Active operation: \{agentAccessStatus\.active_operation\}/);
  assert.match(settings, /Communication trace gaps: \{agentAccessStatus\.trace_gaps\}/);
  assert.match(app, /trace_gaps: 0/);
});

test("Agent Workbench tools are documented with lock and evidence behavior", async () => {
  const guide = await readFile(new URL("../../../docs/AGENT_ACCESS.md", import.meta.url), "utf8");
  assert.match(guide, /Lock\/expiry stops agent-owned CAN scans/);
  assert.match(guide, /\.candump\.csv/);
  assert.match(guide, /same Windows user/);
});
