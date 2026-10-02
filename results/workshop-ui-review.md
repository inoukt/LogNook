# Workshop UI review — Milestone 1 screen, post-refactor revision

Reviewed 2026-10-02. Review only: **no application source was edited**, no vehicle commands were
sent, and no physical-compatibility claim is made. Findings below are limited to observable
behaviour of the revision pinned in §2.

---

## 1. Summary

The screen is now a credible single-view acquisition workspace. The refactor that landed while
this review was queued materially improved it: a real focus-visible treatment exists on the
interactive controls, the palette passes WCAG AA for text almost everywhere, the mode switch is
properly exposed (`aria-pressed`), the reorder buttons give keyboard users a path that the
drag-and-drop gesture alone would deny them, and the per-signal measured rate — which did not
render in the pre-refactor revision (see
[sample-rate-readout-verification.md](sample-rate-readout-verification.md)) — now updates
correctly.

The serious problems are not stylistic. They are all versions of one theme: **the screen cannot
tell a technician when the data it is showing is no longer live, and it cannot show where the
data stopped.** A stalled or unplugged adapter looks exactly like a healthy one — same values,
same green status dot, same "valid" tag, same rate figure — and the chart draws a continuous
line straight across the outage. In a diagnostic tool, that is the failure mode that leads
someone to condemn a working sensor.

`npm run build` passes on this revision: svelte-check reports **0 errors and 0 warnings**, Vite
produces `dist/assets/index-QcRoRJH_.js` (55.6 kB).

## 2. Revision under review

| File | SHA-256 | Modified |
| --- | --- | --- |
| `apps/desktop/src/App.svelte` | `a0a69d0042cbca32eecac0b68df44c8c7bc7c6fd23dc8157d88795ef11c3346b` | 2026-10-02 08:40 |
| `apps/desktop/src/SignalGraphs.svelte` | `71c38779e57dde803b7443295e6f9f7014d7c890186df9c176ce974b219ce247` | 08:39 |
| `apps/desktop/src/LiveChart.svelte` | `a55b818a46842283f8cd06ccbc664e0665e70d1604f81a50899dc6fec5920ac9` | 08:36 |
| `apps/desktop/src/app.css` | `acf2b8878f92354bd644086df89a50c7474a368d767f267cfef216ad5eeafb50` | 08:42 |
| `apps/desktop/src/workspace.ts` | `6627756bbaa38516ec28b1a0715d5fe4433c4d37b5c68c87e159a0c9f064acfc` | 08:42 |
| `apps/desktop/src/types.ts` | `d9dd178d1463dac4f1203c1c8c4209a503b938353171c0d3bb1c7085693ff3b2` | 08:35 |

The tree was being rewritten by a parallel agent during the early part of this review (an earlier
`App.svelte` and `LiveChart.svelte` pair was replaced, and `SignalGraphs.svelte`/`workspace.ts`
were added). Everything reported here is against the hashes above, which were stable from 08:42
onwards, and the freezes/renders were re-taken after that point.

## 3. Runtime, viewport and method

- **Frontend runtime:** the Vite half of the documented dev command — `npm run dev`
  (Vite 8.3.2, `http://127.0.0.1:1420`, the port wired to `tauri dev` via
  `vite.config.ts`'s `strictPort`). The previous dev server had been shut down, so this one was
  started for the review.
- **Window sizes inspected:** `1304 × 821` CSS px (the content area of the configured default
  `1320 × 860` window from `tauri.conf.json`) and `760 × 900` CSS px (the configured
  `minWidth: 760`, i.e. the narrowest window the app allows).
- **Method, and its limits:** the native Tauri window cannot be captured or driven by my
  tooling, so the app's **own components and stylesheet** were mounted in a Chromium tab and
  driven through a temporary in-page implementation of the Tauri IPC bridge (`invoke`/`listen`
  and the `lognook-event` envelope). Every pixel and every behaviour below is the app's real
  code and real CSS; the *sample values* were synthetic, delivered at a known 10 Hz cadence.
  No application file was created or modified to do this, and the bridge lives only in the
  review browser tab.
- **Build check:** `npm run build` — svelte-check **0 errors, 0 warnings**; production build OK.
- **Not inspected:** the native window chrome, OS DPI scaling / fractional scaling, Windows
  High Contrast mode, a screen reader, and anything involving real hardware. Colour and contrast
  figures were computed from the stylesheet values (WCAG 2.1 relative-luminance maths).

## 4. What already works

- **Focus visibility is real now.** `.button`, `.mode-switch button`, `.chart-card` and
  `.graph-order-controls button` carry `:focus-visible { outline: 2px solid var(--accent) }`.
  Verified live: after keyboard Tab, the focused button computed
  `outline: rgb(86, 200, 189) solid 1.333px` and matched `:focus-visible`. Text inputs signal
  focus via a teal border.
- **Contrast is broadly AA.** Every text/background pair measured passes 4.5:1 with room to
  spare (body 13.5:1, muted 10 px hints 6.3:1, the 9 px "GRAPH VIEW" label 5.5:1, chart footer
  5.4:1, all notice variants 9.4–10.7:1). All seven trace colours sit between 6.5:1 and 10.8:1
  against the panel background, so the data itself is legible. The one exception is §5 P7.
- **The mode switch is properly semantic**: `role="group"` with an `aria-label`, plus
  `aria-pressed` on each button.
- **Reordering is not drag-only**: each card has `←`/`→` buttons with `aria-label` and `title`,
  correctly disabled at the ends of the list.
- **Empty and disabled states read clearly.** Disabled controls drop to 45 % opacity and say
  what they need ("Record" is disabled until connected; "Export CSV"/"Replay" until a recording
  exists). The empty state explains the next action.
- **The recording banner is unambiguous**: a red-tinted notice with a pulsing dot, the words
  "Recording live samples", and the full destination path; the Record button becomes a red
  "Stop recording" in the same toolbar position.

## 5. Prioritised findings

### P1 — A dead feed is indistinguishable from live data (highest impact)

With the sample stream stopped for **9 seconds**, the UI showed: values frozen at 834.2 rpm /
87.3 °C / 30.9 °C, both chart footers still reading `Valid` and `9.9 Hz`, the topbar still green
and still reading "Simulated OBD · read only", and **no notice, no warning, and no
stale/age/timeout text anywhere in the document**. A regex for `stale|ago|age|last|timeout|no
data` over the rendered text matched nothing.

Why it matters: on real hardware the equivalent states are a dropped USB link, an ECU that stops
answering (`NO DATA`), and a bus that never initialised (`UNABLE TO CONNECT`). `types.ts`
already models these as `SampleQuality` values (`stale`, `timeout`, `disconnected`) and the
engine can emit `error`/`disconnected` events — but no quality other than `valid` has any visual
treatment, and nothing watches for a *silence* in the first place. A technician reading a frozen
coolant value would reasonably conclude the thermostat is stuck.

Direction: derive a per-signal data age from `wall_time_unix_ns`/`elapsed_ns`, show it (e.g.
"0.3 s" / "8 s ago"), and drive a clearly different treatment once it exceeds a threshold
(greyed value, amber "stale" chip, dimmed status dot). Surface a session-level "no data for
N s" state from the acquisition layer rather than waiting for an error event.

### P2 — Outages are drawn across, and the graph has no scale to read

Two related defects in the same widget:

1. **Gaps are bridged.** After the 9 s outage above, the trace resumed as one continuous
   `<polyline>` with a straight segment spanning the missing period. `chartPoints()`
   (`workspace.ts`) maps each sample's `elapsed_ns` across the visible
   `[min, max]` time window, so the *duration* is proportional and honest, but there is no
   break, no gap marker, and no annotation. The product spec is explicit that a continuous
   misleading line across an outage must not be the default.
2. **Nothing is labelled.** The SVG draws three dashed gridlines and no axis text at all — no
   vertical values, no units, no time labels. Combined view compounds this: each trace is
   normalised to its own min/max with 12 % padding, so a coolant channel that moved 1.5 °C draws
   a wave as tall as a 20 rpm idle wobble. The card does say "each signal uses its own vertical
   scale", which is honest, but a technician still cannot read a magnitude or a timescale off
   either view.

Direction: split the polyline at gaps (or overlay an explicit gap band), and add y-axis
min/max/ticks plus a time axis. If independent auto-scaling stays, label each trace's range in
the legend so the reader knows what the wave means.

### P3 — The "simulated vs physical" badge is hardcoded and will start lying

`App.svelte`'s `connected` handler sets `status = "Simulated OBD · read only"` from a literal. It
ignores the event payload, which already carries `device.stable_id`, `device.hardware_family`,
`protocol` and `read_only`. The status dot uses the same green `.state-dot.online` class any
adapter would use.

This answers the quest's question as: *not yet*. Today it is merely misleading (the only adapter
is the simulator, and the pre-connect button says "Connect simulator"); the moment the
`ObdLinkSxAdapter` from the serial-research brief is registered, the badge will keep saying
"Simulated OBD" while reading a physical SX — an actively wrong claim shown to a technician.
The rendered state also never displays the negotiated protocol or which ECU is talking, both of
which the product spec asks a connection badge to carry.

Direction: render the badge from `ConnectionCapabilities` (device family + protocol + read-only),
keep a distinct, persistent visual marker for *simulated* data (different dot colour and the word
"SIMULATED" in the badge), and make "read only" a first-class badge rather than part of a
sentence.

### P4 — The app overflows and clips at its own minimum window size

At a 760 px CSS viewport the measured layout viewport was **737 px** (the vertical scrollbar takes
the rest) while `body { min-width: 760px }` forced the content to 760 px. Result:
`scrollWidth 760 > clientWidth 737`, a **horizontal scrollbar**, and clipped right-aligned
content — "FOUNDATION POC" in the topbar and "Drag to swap · resize at corner" in the
measurements header were both cut mid-word, and the toolbar's right column ran to the edge.

Because `tauri.conf.json` sets the *outer* window `minWidth: 760`, the webview content area is
always narrower than 760 px, so this is not an edge case: at the smallest window the app always
overflows horizontally, and on a workshop laptop in split-screen a technician will be scrolling
sideways to reach the Record button.

Direction: put the 760 px floor on the layout container rather than the body (or lower it and let
the `@media (max-width: 800px)` rules do the work), add `scrollbar-gutter: stable` so the
scrollbar does not silently subtract from the viewport, and confirm the toolbar and topbar wrap
instead of clipping.

### P5 — Inserting a marker gives no feedback whatsoever

The command reaches the engine correctly (`insert_marker` with
`{label: "Manual marker", note: "radiator fan kicked in"}` was observed arriving), and the input
box clears. Beyond that the UI does nothing: no count, no toast, no marker on the chart, no
marker list. `App.svelte`'s `handleEvent` has no branch for the `marker` event type even though
`types.ts` defines it and the core engine emits `Marker` events.

So a technician can never confirm a marker landed, see how many they took, or find where they
are in the recording. For a feature whose entire value is "something happened at this moment",
silent success is close to useless.

Direction: handle the `marker` event, show a brief confirmation plus a running marker count, and
draw markers on the time axis.

### P6 — Rate and quality are invisible in the default view

The configured default is Combined view, and in that mode **zero** `.chart-footer` elements
exist: no quality tag and no measured rate. The header's right-hand note reads "Combined signals
· shared time" instead. A user must switch to Separate to see whether the data is `valid` or
whether a PID is answering at 10 Hz or 1 Hz — precisely the numbers the project advertises.

Direction: carry quality and rate into the Combined legend (or add a compact per-signal status
strip underneath), so the default view is not the one view that hides data health.

### P7 — Smaller accessibility issues

1. **The only in-chart text fails contrast.** `svg text { fill: #657984 }` on the panel
   `#19232d` measures **3.50:1** — below the 4.5:1 AA threshold for the 11 px text it is used
   for ("Waiting for samples"). Everything else measured passes (see §4).
2. **`aria-label` on plain `div`s is not exposed.** `.chart-legend aria-label="Signal values"`
   and `.graph-order-controls aria-label="Reorder graph"` are ignored by assistive tech without
   a role (`group`/`list`) or a real list semantic.
3. **The chart's accessible name lost detail.** The overlay SVG is labelled
   `"Engine speed, Coolant temperature, Intake air temperature live graph"` — units are gone
   (the previous revision included them), values are absent, and the graph is otherwise
   unreadable to a screen reader. The legend carries the values but is not a list.
4. **The brand link is a reload trap.** `<a class="brand" href="/">` will navigate the webview
   when clicked in the Tauri shell; during a recording that means losing a session's UI state.
   (Not verified in-shell — flagged from source.) A `<button>`-based reset or `href="#"` with a
   handler would remove the risk.
5. **No Space-to-mark shortcut**, which the product spec lists for the live/graph focus. Marking
   currently requires the mouse plus the input field.
6. **Card resizing is mouse-only.** `resize: both` on the separate-mode cards has no keyboard
   equivalent or reset.
7. **Signals cannot be changed while connected.** Checkboxes are disabled when `connected`, so
   adding a PID means disconnecting. That may be deliberate, but nothing tells the user why the
   control is greyed — a one-line hint ("Stop acquisition to change the signal set") would
   prevent the "the app is broken" reaction.

## 6. Accessibility snapshot

| Check | Result |
| --- | --- |
| Text contrast (all rendered pairs) | Pass ≥ 4.5:1, except 11 px SVG text at 3.50:1 (P7.1) |
| Non-text contrast: trace colours vs panel | Pass, 6.5–10.8:1 |
| Non-text contrast: gridlines `#2d3c47`, borders `#30414e` | 1.40 / 1.51:1 — decorative, but they are the only scale reference in the chart (see P2) |
| Keyboard reachability | All controls are real buttons/inputs; drag-only reordering has a keyboard alternative |
| Focus visibility | Pass — teal `:focus-visible` outline verified programmatically and visually |
| Labels | `aria-label` present on search, marker note, mode switch, reorder buttons; ineffective on two plain divs (P7.2) |
| State announcements | Mode switch uses `aria-pressed`; the error notice uses `role="alert"`. Success/recording notices do not |
| Zoom / reflow | At the minimum window width the layout reflows to one column but overflows horizontally (P4) |

## 7. The single highest-value fix before Milestone 2

**Make the screen tell the truth about data that has stopped arriving.** Concretely: a per-signal
data-age/staleness indicator (P1) plus a visible break or gap band in the chart instead of an
interpolated segment (P2).

It is the highest-value fix because every other finding is about clarity or polish, while this
one is about correctness of interpretation: an unmarked outage turns "no data" into confident
wrong numbers, and it is the failure a technician is most likely to act on. It also happens to be
the cheapest of the high-impact items — the data needed (`wall_time_unix_ns` per sample, and the
`SampleQuality` enum that already exists) is already flowing to the UI. The blueprint's own
requirement — "On reconnect, record a visible gap and require reacquisition of actual data;
never draw a continuous misleading line across outages by default" — is currently unmet.

## 8. Not tested

- The native Tauri window, its title bar, and OS window-manager behaviour (no capture path).
- DPI scaling, Windows High Contrast, and a real screen reader.
- Any real adapter, driver, COM port, or vehicle.
- Sustained multi-hour sessions, very long recordings, and the replay path under load
  (the replay/stop-replay toggle and "Replayed recording" heading were only reviewed in source).
