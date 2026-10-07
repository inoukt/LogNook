import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("../src/OemCatalogPanel.svelte", import.meta.url), "utf8");
const rule = (selector) => source.match(new RegExp(`${selector}\\s*\\{([^}]+)\\}`))?.[1] ?? "";

test("OEM search results stay in a vertical-only column without covering cards", () => {
  const list = rule("\\.oem-catalog-list");
  const candidate = rule("\\.oem-candidate");

  assert.match(list, /grid-template-columns:\s*minmax\(0,\s*1fr\)/);
  assert.match(list, /min-width:\s*0/);
  assert.match(list, /overflow-x:\s*hidden/);
  assert.match(list, /overflow-y:\s*auto/);
  assert.match(list, /scrollbar-gutter:\s*stable/);
  assert.match(candidate, /min-width:\s*0/);
});

test("OEM research uses the shared heading and compact, single-line candidate rows", () => {
  const candidateTitle = rule("\\.oem-candidate > header strong");
  const candidateHeader = rule("\\.oem-candidate > header");

  assert.doesNotMatch(source, /<h3>OEM research catalog<\/h3>/);
  assert.match(source, /Source-backed candidates · research only/);
  assert.match(candidateTitle, /white-space:\s*nowrap/);
  assert.match(candidateHeader, /gap:\s*4px/);
  assert.match(rule("\\.oem-research"), /gap:\s*4px/);
});
