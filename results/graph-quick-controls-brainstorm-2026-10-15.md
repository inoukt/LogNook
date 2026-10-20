# Compact graph controls — approved design

Implementation was authorized later on 2026-10-15. See
[the implementation report](graph-quick-controls-2026-10-15.md) for delivered
behavior and verification; the notes below preserve the approved design.

The owner selected Y zoom of the focused trace, chosen by clicking its legend.

Proposed compact strip inside the existing Graph settings bar:

`Graph [Time series ▼]   X [−] [+]   Y [−] [+]   [Auto]`

- Keep one bar. Use small buttons (about 24 px), accessible names and tooltips.
  Shorten the long Graph settings subtitle to leave space; collapse secondary
  controls into a small overflow menu at narrow widths.
- Time-series X buttons change the time span. Keep the right edge anchored to
  current live data; use the playback cursor as the replay zoom anchor.
- Time-series Y buttons adjust the focused trace's existing manual range. A
  clear legend outline shows the focus; the tooltip names the affected signal.
  In separate mode, each chart can use its own focused trace automatically.
- Scatter X/Y buttons adjust the corresponding signal range. Reuse existing
  per-signal manual bounds rather than adding a second scaling system.
- Each click changes span by a small fixed factor, such as 1.25; retain finite,
  nonzero bounds and the existing time-span limits.
- Auto restores automatic value bounds for the current plot/focused trace. For
  X, return to the configured live span or fit the replay selection. A later
  implementation should make that reset target explicit in its tooltip.
- Optional later controls: pan left/right when replaying or the display is
  frozen, and a pause-display button that keeps acquisition/recording running.
  Avoid adding all of these to the first compact strip.
- Keep the current scatter point cap and display batching. Zoom changes the
  view, not measurements or adapter polling.

Implementation should cover focused-trace removal, constant/negative ranges,
replay boundaries and restored settings. The feature introduces no adapter
commands.
