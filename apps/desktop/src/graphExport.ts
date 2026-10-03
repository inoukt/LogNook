export type GraphExportLegendItem = { color: string; label: string; value: string };

export function graphPngFilename(date: Date): string {
  const timestamp = date.toISOString().replace(/[-:.]/g, "");
  return `lognook-graph-${timestamp}.png`;
}

export function buildGraphExportSvg(
  graphMarkup: string,
  options: { width: number; height: number; title?: string; legend?: readonly GraphExportLegendItem[] },
): { svg: string; width: number; height: number } {
  if (!Number.isFinite(options.width) || options.width <= 0 || !Number.isFinite(options.height) || options.height <= 0) {
    throw new RangeError("Graph export dimensions must be positive numbers.");
  }

  const width = Math.round(options.width);
  const headerHeight = options.title ? 48 : 0;
  const graphHeight = Math.round(options.height);
  const legend = options.legend ?? [];
  const legendHeight = legend.length ? 32 + legend.length * 48 : 0;
  const height = headerHeight + graphHeight + legendHeight;
  const titleMarkup = options.title ? `<text class="export-title" x="24" y="32">${escapeXml(options.title)}</text>` : "";
  const legendMarkup = legend.map((item, index) => {
    const y = headerHeight + graphHeight + 48 + index * 48;
    const color = /^#[\da-f]{6}$/i.test(item.color) ? item.color : "#56c8bd";
    const label = escapeXml(item.label);
    const value = escapeXml(item.value);
    const maxLabelWidth = Math.max(1, width - 230);
    const labelTextLength = label.length * 9 > maxLabelWidth ? ` textLength="${maxLabelWidth}" lengthAdjust="spacingAndGlyphs"` : "";
    return `<circle cx="24" cy="${y - 7}" r="8" fill="${color}"/><text class="legend-label" x="44" y="${y}"${labelTextLength}>${label}</text><text class="legend-value" x="${width - 20}" y="${y}" text-anchor="end">${value}</text>`;
  }).join("");

  return {
    width,
    height,
    svg: `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}"><style>line{stroke:#2d3c47;stroke-width:1;stroke-dasharray:3 5}.marker-guide{stroke:#f0d45c;stroke-width:1.3;stroke-dasharray:2 3;vector-effect:non-scaling-stroke}.marker-cap{fill:#f0d45c}.playhead-guide{stroke:#56c8bd;stroke-width:1.5;stroke-dasharray:5 3;vector-effect:non-scaling-stroke}.reference-trace{stroke-dasharray:6 4}polyline{fill:none;stroke-width:2.3;stroke-linecap:round;stroke-linejoin:round;vector-effect:non-scaling-stroke}text{fill:#657984;font:22px Inter,'Segoe UI',sans-serif}.export-title{fill:#e7edf3;font-size:20px;font-weight:600}.legend-label,.legend-value{fill:#aab9c2;font-size:16px}</style><rect x="0" y="0" width="${width}" height="${height}" fill="#19232d"/>${titleMarkup}<svg x="0" y="${headerHeight}" width="${width}" height="${graphHeight}" viewBox="0 0 720 168" preserveAspectRatio="none">${graphMarkup}</svg>${legendMarkup}</svg>`,
  };
}

function escapeXml(value: string): string {
  return value.replace(/[&<>"']/g, (character) => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&apos;",
  })[character]!);
}
