# Compact graph controls — 2026-10-15

Implemented with Luna xhigh implementation and review agents in the existing
`codex/can-event-freeze-frame` worktree. Prior local changes were preserved.

## Use

The existing Graph settings bar now contains `X −/+`, `Y −/+`, and `Auto`.
Controls remain visible while the detailed settings are collapsed.

- In combined time-series view, click a signal in the graph legend to select
  the Y target. The selected entry has a highlighted border. In separate view,
  click the chart title instead. Tooltips name the affected signal.
- X changes the live time span around the right edge or uses the existing
  replay zoom around the playback cursor. Each step changes the span by 1.25.
- Y changes the selected trace's existing manual minimum/maximum around the
  displayed range center. Constant, negative and large finite ranges are
  guarded against zero-span collapse.
- In X/Y scatter view, X and Y zoom the respective configured signal axes,
  using existing per-signal manual bounds and the same bounded, aligned points.
- Auto restores automatic bounds for the selected trace (both axes in scatter).
  For time-series it also restores the configured live span or fits the full
  recording during replay. Its tooltip names the reset target.
- Removing the focused trace falls back to the first remaining trace.
  Live zoom factor and signal bounds persist with the graph preferences.

No acquisition, hardware request, recording or stored measurement operation
was added or changed. These are renderer-local presentation controls; there
is no meaningful separate MCP operation, consistent with existing manual graph
scales. Existing scatter display caps and batched sample delivery remain.

## Review and verification

A separate Luna xhigh reviewer found a recovery problem when replay zoom landed
in an interval without samples. The implementation was revised so replay X
and Auto reset do not depend on visible samples. Scatter pairing is skipped
when the time-series graph is selected.

Final verification on the revised tree:

- `pnpm.cmd run test:ui`: **230 passed, zero failed**. [Test log](graph-quick-controls-ui-tests-2026-10-15.txt).
- `pnpm.cmd run build`: **zero Svelte errors/warnings**, Vite production build passed. [Build log](graph-quick-controls-ui-build-2026-10-15.txt).
- `git diff --check`: passed.
- Scoped Luna xhigh re-review confirmed empty-window recovery was fixed, with
  no remaining finding. Native desktop visual acceptance was not performed.

No physical vehicle testing, commit or push was performed for this change.

Release build passed:
`cargo build -p lognook-desktop --release --features tauri/custom-protocol --locked -j 4`.
[Release build log](graph-quick-controls-release-build-2026-10-15.txt).
Artifact: `C:\Users\inouk\LogNook-can-event-freeze-frame\target\release\lognook-desktop.exe`,
32,361,984 bytes, modified 2026-10-16 02:41:05 UTC (October 15 local time).
The release embeds the built UI and needs no development server. Bundling remains
disabled; no installer was produced. The existing release OpenPort helper is
unchanged by this UI feature. The app was left closed for the owner to launch.
