export function adjustResearchResultsHeight(heightPx: number, deltaPx: number, viewportHeightPx: number): number {
  const maximum = Math.min(360, viewportHeightPx * 0.4);
  const minimum = Math.min(120, maximum);
  return Math.max(minimum, Math.min(maximum, heightPx + deltaPx));
}
