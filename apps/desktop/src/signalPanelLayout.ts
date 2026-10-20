export const DEFAULT_SIGNAL_PANEL_WIDTH = 340;
export const MIN_SIGNAL_PANEL_WIDTH = 240;
export const MAX_SIGNAL_PANEL_WIDTH = 640;

export function clampSignalPanelWidth(width: number, workspaceWidth = 0): number {
  // Reserve the list toggle, two gaps and 320px for the graph on wide layouts.
  const maximum = Number.isFinite(workspaceWidth) && workspaceWidth > 0
    ? Math.max(MIN_SIGNAL_PANEL_WIDTH, Math.min(MAX_SIGNAL_PANEL_WIDTH, workspaceWidth - 362))
    : MAX_SIGNAL_PANEL_WIDTH;
  return Math.round(Math.max(MIN_SIGNAL_PANEL_WIDTH, Math.min(maximum,
    Number.isFinite(width) ? width : DEFAULT_SIGNAL_PANEL_WIDTH)));
}
