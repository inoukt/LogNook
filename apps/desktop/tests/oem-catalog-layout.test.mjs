import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("../src/OemCatalogPanel.svelte", import.meta.url), "utf8");
const rule = (selector) => source.match(new RegExp(`${selector}\\s*\\{([^}]+)\\}`))?.[1] ?? "";

test("OEM search results stay in a vertical-only column without covering rows", () => {
  const list = rule("\\.oem-catalog-list");
  const candidate = rule("\\.oem-candidate-row");

  assert.match(list, /flex-direction:\s*column/);
  assert.match(list, /min-width:\s*0/);
  assert.match(list, /overflow-x:\s*hidden/);
  assert.match(list, /overflow-y:\s*auto/);
  assert.match(list, /scrollbar-gutter:\s*stable/);
  assert.match(candidate, /min-width:\s*0/);
});

test("OEM research keeps request summaries clipped within compact rows", () => {
  const candidateSummary = rule("\\.candidate-inspect small");

  assert.doesNotMatch(source, /<h3>OEM research catalog<\/h3>/);
  assert.match(source, /Source-backed candidates · research only/);
  assert.match(candidateSummary, /white-space:\s*nowrap/);
  assert.match(candidateSummary, /text-overflow:\s*ellipsis/);
  assert.match(rule("\\.oem-research"), /gap:\s*4px/);
});
