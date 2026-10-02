# Product and user experience

## Primary 20-second workflow (target, not guaranteed on all cars)
Open app -> automatically offer previously used adapter/vehicle/workspace -> show verified connection/capability badge -> suggested preset or searchable signal picker -> Start. Connection status must distinguish USB present / hardware opened / bus detected / ECU responding / signals actively updating. No splash delay or blocking dashboard.

## Modes
- **Live**: compact status line, accessible Connect/Record/Marker, searchable/favorite signals, live values and synchronized graphs, two-pane graph+list, single pane graph-only, detachable/fullscreen panels.
- **Analyze**: recording browser, CSV import, synchronized cursor, markers/comments, overlays, range statistics, graphs, scatter, heatmap, bars, histograms, event timeline, delta views.
- **Inspect**: ECU/source tree, available/unsupported signals, raw CAN frame view, DTC read/report in a later milestone, communication diagnostics/trace for advanced users.
- **Settings**: device preferences, keyboard mapping, units, formula editor, sound, privacy, themes/backgrounds, workspace reset, accessibility.

## Signal picker
Discover what the active car actually supports; search by name/alias/category/ECU/unit; favorites, recent PIDs, drag-to-pin and presets (generic service, cold-start, misfire observation, boost/tuning, EV, J1939 when supported). Display real sample frequency, data age, refresh priority, signal tooltip and source. 'Requested but unavailable' is explicit, never a fabricated zero. Show likely sample-rate cost before applying a broad preset.

## Recording and replay
Single-key record/stop and Space marker (only in graph/live focus; ignored while typing); editable, deletable timestamped markers with notes and optional severity/color; trigger pre/post-roll; auto-record on selected conditions; one-click pause live display without stopping acquisition; replay with speed control, scrubbing, bookmarks, variable highlighting, frame-accurate raw-data inspection and synchronized multiple recordings. Confirm before destructive deletion; support undo where feasible.

## Graph workspace
Line/time series, stacked synchronized timelines, overlaid or independent Y axes, XY/scatter with color-by-third-variable, heatmaps/binning, bar/histogram, derivatives and difference charts. Cursor crosshair, zoom, selection, min/max/average/percentiles, axis auto/manual/lock, chart/series colors, annotations, visible-range export and screenshot. Persist the last valid manual range and last color PER `signal_key + unit + workspace` so switching °C/°F does not silently reuse inappropriate limits. Display an explicit indicator when auto-range is in force. Graph/list ratio resizable; graph-only shortcut; light, slate/dark-high-contrast and user-selected backgrounds; warn if wallpaper reduces text contrast.

## Calculations, good signals, assistance
Store raw values and apply non-destructive unit conversion, tare, offsets, filtering, derivatives, math formulas (safe parsed expression language, no arbitrary execution); version formulas and detect incompatible units. Known-good library stores VIN-redacted optional labels: model, year, engine, ECU/software variant, climate/load/test procedure, required channels and quality note. Overlay by time, event, RPM or selected anchor; never portray unaligned asynchronous points as simultaneous. Contextual hover help for controls and PIDs, with manufacturer-variant notes and citation/source provenance.

## Technician time savers
Last workspace restored automatically; one-click temporary clean workspace; remembered per-vehicle presets; duplicate recording and notes; session tags/customer/job optional; battery voltage when supported; automatic protocol diagnosis; clear error explanations with suggested recovery; built-in self-test/synthetic replay; clipboard screenshot with privacy masking; automatic filename; shortcuts fully remappable, including conflict detection; recent projects and offline help.

## Accessibility and operational clarity
Keyboard-only navigation, minimum contrast requirements, configurable font scaling, prominent stale-data and no-connection indicators, audible-alert mute and documented sound limits. On reconnect, record a visible gap and require reacquisition of actual data; never draw a continuous misleading line across outages by default.

## Existing-product benchmark and differentiation
Datazap already offers synchronized charts, scatter/heatmaps, math channels and overlays; SavvyCAN already offers DBC-oriented raw CAN and UDS exploration; RomRaider is established for Subaru-focused logging/tuning, contingent on separately obtained ECU/logger definitions; asammdf handles substantial MDF/MF4 measurement-analysis workloads. LogNook must not claim its chart types alone are unique. Its product distinction is a low-friction, offline-first, *cross-adapter live acquisition* workflow joined to rigorous timing/provenance, advanced graphing, persistent shop workspaces and reliable export. Compare measured technician task completion times and reliability against these applications; do not copy incompatible-licensed source code.

For multiple shop PCs, first provide versioned workspace/profile import-export; later optional cloud sync should synchronize finalized sessions and versioned profile snapshots rather than an open SQLite recorder database.
