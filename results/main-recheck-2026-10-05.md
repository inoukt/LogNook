# Main re-check — project state and Oct-2 review findings, re-verified against `main`

Re-check performed 2026-10-05. **Read-only:** every claim below comes from git object access
(`git show` / `git grep` against `main`), not from a working-tree checkout. The working tree was
left untouched — it is on `codex/oem-candidate-try` and contains another agent's in-progress edits
(`PROJECT_STATUS.md`, `OemCatalogPanel.svelte`, a new untracked test), which were not read, staged,
or modified. No vehicle commands were sent. No build or test run for `main` was performed this
pass (see §6).

## 1. Revisions

| Ref | Commit | Subject |
| --- | --- | --- |
| `main` (re-check target) | `3a59172` | fix: import FORScan wide CSV logs (2026-10-04 15:50) |
| HEAD (`codex/oem-candidate-try`) | `51a1c88` | main + 4 commits (OEM candidate try work), plus uncommitted changes |

All six UI files pinned in `workshop-ui-review.md` §2 have **different hashes on `main`** — the
reviewed Oct-2 revision is no longer the current UI at any level. Findings below were therefore
re-derived from the `main` sources themselves, not from the earlier screenshots.

## 2. What `main` is now

Since the Oct-2 review, `main` has become the **OBDLinkSX-Beta1 (0.1.0-beta.1)** release line:

- `9f2e907` release: OBDLinkSX-Beta1; then the OpenPort 2 Mode-01 stack landed end-to-end
  (typed Mode-01 parsing → bounded J2534 worker protocol → isolated J2534 worker → Mode-01
  acquisition adapter → testing controls), with `77a3e6c` fixing the worker to follow the
  ISO 15765 J2534 receive contract, plus FORScan wide-CSV import fix (`3a59172`).
- `PROJECT_STATUS.md` @ main records SX live read-only acquisition verified on the Ford MSI test
  machine (5-PID stuffing 80.5 values/s; 7 groups 350/350 valid at 81.42 values/s; bare ~16.6/s;
  first live RPM read 632 rpm), 35/41 advertised PIDs requestable, six review-required
  (`01,15,19,34,38,41`), and the OpenPort 2 worker compiling but unvalidated on hardware.
- The SX OEM candidate work (`oem_probe.rs`, candidate-try actions, A30 docs) exists only on HEAD
  and is not yet part of `main`.

## 3. Oct-2 findings, re-verified at `main`

| # | Oct-2 finding | Status @ `main` | Evidence |
| --- | --- | --- | --- |
| 1 | Dead feed indistinguishable (frozen values, green dot, `Valid`) | **OPEN** | `Stale` exists only as an enum (`crates/lognook-core/src/model.rs:36`, `apps/desktop/src/types.ts:87`); every producer stamps `Valid` (`obdlink_sx.rs:393`, `openport2.rs:253`, `pipeline_worker/derived.rs:56`, `csv_io.rs:347`). No liveness watchdog/age display anywhere in `apps/desktop/src`. |
| 2 | Outages drawn continuously — no gap breaks in charts | **OPEN** | Chart is one `<polyline>` built from all points (`LiveChart.svelte:209`); `graphCoordinates` maps samples with no gap logic and `boundedGraphSamples` only filters non-finite values (`graphPreferences.ts:218`). `holdLastPointAtRightEdge` deliberately extends the last sample to x=720, so a stalled feed stays full-width and frozen. |
| 3 | No chart axis labels | **OPEN** | SVG contains gridlines at y=22/84/146 and no `<text>` at all (`LiveChart.svelte:195-246`). No value or time tick labels were added. |
| 4 | Combined mode normalizes each trace against itself | **UNCHANGED (now disclosed)** | Each trace still computes its own `graphValueRange` (`LiveChart.svelte:79`); the panel header now says "Shared time axis · each signal uses its own vertical scale" (`SignalGraphs.svelte:270`). |
| 5 | Badge hardcoded `"Simulated OBD · read only"` | **FIXED** | `App.svelte:342-344` derives it from `device.hardware_family`; simulator text only for the simulator. |
| 6 | Markers write-only | **FIXED** | Live marker events applied to the session (`App.svelte:321-327`), playback marker add/update/delete invokes (`App.svelte:758-772`), note input, and markers rendered as guides + caps on the chart (`LiveChart.svelte:196-199`). |
| 7 | Overflow at the 760 px minimum width | **LIKELY FIXED (source)** | `body` is now `{ margin: 0; min-height: 100vh; }` — the `min-width: 760px` rule that caused the 737 px layout overflow is gone (`app.css:73`); toolbar fields use clamps with min-widths and responsive breakpoints at 980/900 px. Not visually re-confirmed this pass. |
| 8 | Sample-rate readout (verified fixed Oct-2) | **STILL PRESENT** | `SignalGraphs.svelte:304` renders `{trace.sampleRate}` in the per-trace footer; still derived from the last two samples, so it freezes rather than decays on an outage — must not be used as a liveness signal. |

## 4. What this means

The two findings that were pure wiring defects — the hardcoded badge and write-only markers — are
resolved on `main`. The three that matter diagnostically are untouched: **a stalled feed still
looks live** (no producer emits `Stale`, no age display, frozen-but-`Valid` values), **outages are
still drawn as continuous lines**, and **the charts still carry no axis labels**. Per-trace scaling
in the combined view remains but is now captioned in the UI.

## 5. Method and safety

- `git show main:<path>` and `git grep <pattern> main -- <paths>` only; no `checkout`, no
  worktree, no stash, no writes to tracked files. New: this report file.
- Head and branch re-confirmed before and after: `codex/oem-candidate-try` @ `51a1c88`, same
  three dirty/untracked entries, untouched.

## 6. Not re-verified this pass

- No `npm run build`, `npm run test:ui`, or `cargo test --workspace` was run **against `main`**.
  Running them would require checking out `main` (would disturb the other agent's uncommitted
  work) or an isolated worktree plus the staged Rust environment; neither was done without a
  go-ahead.
- No visual re-run of the browser-mock harness against the current revision. Everything pixel-
  level from the Oct-2 review (contrast, focus rings, 760 px behaviour, frozen-feed appearance)
  should be considered *source-reviewed, not re-observed* until someone re-runs it against a
  pinned hash set.

## 7. Suggested next validations

1. Add a liveness/age signal (UI watchdog or a core producer that emits `Stale` when a trace stops
   arriving) — the highest-value open finding.
2. Break chart polylines on time gaps and add minimal axis labels (range min/max, time ticks).
3. Re-run the visual harness against a freshly pinned `main` revision once the tree is quiet.
