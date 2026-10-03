import assert from "node:assert/strict";
import test from "node:test";
import * as graphExport from "../src/graphExport.ts";

test("graph export SVG retains plotted marks and renders an escaped legend", () => {
  assert.equal(typeof graphExport.buildGraphExportSvg, "function");
  if (typeof graphExport.buildGraphExportSvg !== "function") return;
  const result = graphExport.buildGraphExportSvg(
    '<line class="marker-guide"/><line class="playhead-guide"/><polyline points="0,0 10,10" stroke="#56c8bd"/>',
    { width: 960, height: 240, title: "A&B <graph>", legend: [{ color: "#56c8bd", label: "A&B <sensor>", value: "1.2 °C" }] },
  );

  assert.equal(result.width, 960);
  assert.equal(result.height, 368);
  assert.match(result.svg, /class="marker-guide"/);
  assert.match(result.svg, /class="playhead-guide"/);
  assert.match(result.svg, /<polyline points="0,0 10,10" stroke="#56c8bd"\/>/);
  assert.match(result.svg, /\.reference-trace\{[^}]*stroke-dasharray/);
  assert.match(result.svg, /A&amp;B &lt;sensor&gt;/);
  assert.match(result.svg, /A&amp;B &lt;graph&gt;/);
  assert.match(result.svg, /1\.2 °C/);
  assert.match(result.svg, /<rect[^>]+fill="#19232d"/);
});

test("PNG filenames use a filesystem-safe UTC timestamp", () => {
  assert.equal(typeof graphExport.graphPngFilename, "function");
  if (typeof graphExport.graphPngFilename !== "function") return;
  assert.equal(graphExport.graphPngFilename(new Date("2026-10-03T14:05:06.789Z")), "lognook-graph-20261003T140506789Z.png");
});
