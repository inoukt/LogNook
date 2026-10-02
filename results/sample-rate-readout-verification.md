# Verification: "Measured sample rate" readout

Requested follow-up to the workshop UI review. Question: **does the per-signal measured sample
rate readout ever update?**

Verdict: **it was broken in the pre-refactor Milestone 1 revision, and it is correct in the
revision that is on disk now.** No fix is outstanding; the item can be closed, with two caveats
recorded in §4.

---

## 1. Method

Both revisions were driven through the app's own components with a temporary in-page
implementation of the Tauri IPC bridge (no application file was created or modified; see the
method note in [workshop-ui-review.md](workshop-ui-review.md) §3). The feed was synthetic and its
cadence is known exactly: one sample per selected signal every 100 ms, each carrying
`elapsed_ns` derived from `performance.now()`. The footer text was read from the DOM rather than
screenshotted, and the emitted sample objects were independently inspected to confirm the real
inter-sample deltas, so a display failure could not be confused with a data failure.

This mattered: an early harness of mine published events on the wrong Tauri channel and produced
a UI that updated *nothing at all*, which would have looked like an app bug. Every observation
below was therefore taken with the event channel (`lognook-event`, `{type, payload}` envelope)
confirmed to be delivering first.

## 2. Pre-refactor Milestone 1 revision — reproduced failure

Runtime: `1304 × 821` CSS px (default window content area). Feed: 100 ms, three signals
(`engine_rpm`, `coolant_temp`, `intake_temp`).

| Observation | Measured value |
| --- | --- |
| Points rendered in the RPM curve | 133 (132 for the other two) |
| Real inter-sample deltas in the emitted samples | 101,300,000 ns and 98,700,000 ns ⇒ ~9.9 Hz |
| Value readout | updating normally (e.g. 812.5 → 801.9 rpm) |
| Curve | visible and moving |
| Footer quality span | `valid` (updates) |
| **Footer rate span** | **`— Hz`, unchanged indefinitely** |

The rate span stayed at `— Hz` while the same footer's quality span — rendered from the same
fragment — progressed from `waiting` to `valid`, and while the chart was drawing a full curve
from a series array holding 130+ points per signal. `— Hz` is what `sampleRate()` returns from
its two guard clauses: fewer than two points, or a non-positive delta. Neither condition was
true of the data, so the text was not being re-evaluated.

**Root cause (inference, not proven).** In that revision the rate was computed inside the
template — `{sampleRate(signal.id)}` — and `sampleRate()` reads the component's reactive `series`
object. Svelte 5 compiles some template expressions as *untracked* reads; the compiled output of
the next revision (retrieved from the dev server at 08:36) shows exactly that pattern:
`$.set_text(text_14, ($.get(trace), $.untrack(() => $.get(trace).sampleRate)))`. If the same
treatment was applied to a *function call* whose argument was only the (constant) signal id, the
text would be written once — while `series` was still empty — and never invalidated. The
Milestone 1 sources no longer exist on disk (`App.svelte` and `LiveChart.svelte` were replaced,
and `dist/` was rebuilt), so this mechanism is consistent with the evidence rather than directly
confirmed.

## 3. Settled revision — correct

Same harness, same 100 ms feed, at the same viewport, against `App.svelte`
`a0a69d00…`, `SignalGraphs.svelte` `71c38779…`.

| Observation | Measured value |
| --- | --- |
| Real inter-sample delta | 98,900,000 ns ⇒ ~10.1 Hz |
| Footer spans, Separate view | `valid` / `10.1 Hz` on all three cards |
| Earlier captures, same session | `10.1 Hz`, `10.0 Hz`, `9.9–10.0 Hz` |

The rate is now computed in the `$: traces = …` statement and carried on each trace object, so
the value is produced whenever `series` changes and the template read is a plain property access.
That is the shape that works, and it matches the observed output.

## 4. Caveats worth keeping

1. **It is only visible in Separate view.** The default is Combined, and Combined renders no
   `.chart-footer` at all — so a fresh user sees no measured rate until they change the graph
   mode. (Reported as finding P6 in the UI review.)
2. **It cannot indicate staleness.** The value is derived from the last two samples only, so when
   the feed stops it holds its last figure indefinitely instead of decaying — a frozen `9.9 Hz`
   looks perfectly healthy on a dead bus. That is why P1 (no staleness indicator) remains the
   top finding in the review; the rate readout must not be mistaken for a liveness signal.
   It is also mildly jittery by construction: a single unlucky pair moves the displayed figure by
   ±0.2 Hz.

Optional hardening, since the maths is now isolated and pure: move `sampleRate()` into
`workspace.ts` alongside `chartPoints()` and cover it with unit tests for the two guard clauses,
the zero-delta case, and a known cadence — that would have caught the earlier regression without
a live feed.
