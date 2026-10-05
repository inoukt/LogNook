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
