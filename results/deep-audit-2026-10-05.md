# Deep audit — 2026-10-05 (second pass)

Second, deeper pass over `main` (`3a59172`) and the working tree. Sources only read, plus one
temporary benchmark test that was removed after running (numbers below). The earlier pass is
[bug-hunt-2026-10-05.md](bug-hunt-2026-10-05.md); this pass adds two data-loss/performance findings
that are larger than anything in it, plus a main-thread command-execution issue that amplifies both.

Suites were re-confirmed green before this pass: `cargo test --workspace` (all targets, 70 core
lib tests) and 67/67 tracked UI tests.

---

## H1 — Live CSV export exports the *render buffer*: last 180 samples per signal

**Where:** `apps/desktop/src/App.svelte:316` and `:599`; `apps/desktop/src/liveCsv.ts`.

The live series shown in the graph is intentionally capped for rendering:

```ts
series = { ...series, [sample.signal_id]: [...(series[sample.signal_id] ?? []), sample].slice(-180) };
```

and the live export uses that same capped series:

```ts
const samples = liveCsvSamples(series, logSignalIds);
await invoke<string | null>("export_live_csv", { samples });
```

So "Export live CSV" silently exports only the **last 180 samples per signal** — a buffer chosen
for chart resolution, not for data retention. At the documented SX rates this is:
- single-PID ~33 samples/s → ~5.5 seconds of history;
- 5-PID stuffing ~80.5 values/s (≈16/s per signal) → ~11 seconds;
- fast multi-group rates (81+ values/s per signal on 7 groups) → ~2 seconds.

The backend would accept up to 100,000 samples (`MAX_LIVE_EXPORT_SAMPLES`), and the UI does say
"Saved N buffered samples", but the number is in the hundreds while the user believes they exported
a session. This is a data-loss trap for anyone who logs a test drive without recording.

**Suggested fix:** keep a separate export buffer (e.g. ring buffer up to 100k samples) from the
180-point render buffer, or export from the active/in-progress recording, or make the live export
default to "start recording first". At minimum, show the exported time span, not just the count.

## H2 — Recording read path is O(rows) per request, and it runs on the UI main thread

**Where:** `crates/lognook-core/src/recording_view.rs:7` (`inspect_recording`),
`crates/lognook-core/src/graph_window.rs:20` and `:110` (both call it on every query),
`apps/desktop/src/workspace.ts:25` (refresh cadence), and the sync Tauri commands in
`apps/desktop/src-tauri/src/commands/player.rs` (`recording_window`, `recording_values_at`,
`recording_averages`).

`inspect_recording` runs `COUNT(*)`, a `MAX(elapsed_ns)` over a `UNION ALL` subquery (forces full
scan of `samples`), a `DISTINCT signal_id, unit` scan, and a markers query. It is called on **every**
`query_recording_window` and `query_recording_averages`, only to clamp `start_ns`/`end_ns` against
the duration. During replay the UI refreshes at 100 ms, and while scrubbing at 33 ms
(`graphRefreshIntervalMs`), issuing a window + averages pair each time.

Measured on a synthetic 200k-row / 5-signal / 4000 s recording (debug build, warm file cache):

| Call | Time |
| --- | --- |
| `inspect_recording` | 250–390 ms |
| `query_recording_window(0, end, 360)` (includes inspect) | 630–930 ms |
| `query_recording_averages` (includes inspect) | 360–470 ms |
| `query_recording_values_at` at the **start** | ~0.8 ms |
| `query_recording_values_at` at the **end** | **~310 ms** |

The last row is a separate problem: there is no index on `(signal_id, elapsed_ns)`, so the
"latest value at cursor" query sorts all earlier rows of that signal instead of seeking. It runs on
every seek and on every wheel-scrub step, i.e. up to ~30 times/s.

`list_recordings` (`apps/desktop/src-tauri/src/commands/sessions.rs:14`) also calls
`inspect_recording` once **per recording file**, so opening the session browser scans every
recording in full.

And because the commands are sync, per the Tauri v2 docs they run **on the main thread**
("Commands without the async keyword are executed on the main thread … Asynchronous commands are
preferred in Tauri to perform heavy work in a manner that doesn't result in UI freezes or
slowdowns"). Scrub a large recording and the webview event loop is busy in SQLite for hundreds of
milliseconds per frame.

**Suggested fixes, in impact order:**
1. Persist `duration_ns`, `sample_count`, and the signal list in the `metadata` table at
   `RecordingWriter::finish()` (and for imports); have `inspect_recording`/`list_recordings` read
   that instead of scanning. Window/averages then become O(1) setup.
2. Add `CREATE INDEX samples_by_signal_time ON samples(signal_id, elapsed_ns)` (schema version bump;
   or create it lazily when opening a recording for read-write).
3. Mark the recording-query commands `async` (or `#[tauri::command(async)]`) so they never block
   the webview thread.
4. Optional: bucket in SQL rather than pulling all rows into Rust.

Caveat: numbers are from an unoptimized (debug) build; release will be several times faster. The
asymptotics and the main-thread placement are what matter — at 1M rows the same paths are ~5× these
figures.

## H3 — Long-running sync commands freeze the app (connect, disconnect, probe, export, listing)

**Where:** `apps/desktop/src-tauri/src/commands.rs`, `commands/openport2.rs`,
`commands/oem_catalog.rs`, `commands/sessions.rs`, `commands/exports.rs`.

Only `open_session_file` is `async`; everything else is sync, hence main-thread:

- `connect_obdlink_sx` — serial discovery (identity + up to 8 supported-PID pages + protocol +
  header setup + re-discovery), seconds of blocking I/O with a 2 s per-command timeout.
- `disconnect` — joins the acquisition worker, which can wait for the current serial read
  (`RESPONSE_TIMEOUT` = 2 s) and for the pipeline queue to drain.
- `identify_obdlink_sx` / `probe_scion_xb_mode21` — serial round-trips.
- `available_openport2_providers` / `identify_openport2` — spawn a worker process and wait up to
  3 s.
- `export_csv` — walks and writes the entire recording to CSV **while holding the engine mutex**
  (`commands.rs:142-152`); `share_recording_csv` / `save_recording_as` also write the whole CSV on
  the main thread.
- `list_recordings` — see H2.

During each of these the window cannot repaint. The worst combination is a failed adapter attempt:
connect then disconnect can hold the event loop for several seconds.

**Suggested fix:** make blocking commands `async`/`#[tauri::command(async)]` (works with
`State<'_, _>` when the return type is a `Result`), and drop the engine guard before long I/O where
possible. This is a small, mechanical change with a large responsiveness payoff.

## H4 — After an adapter drop, the chart keeps showing the last session as if current

**Where:** `apps/desktop/src/App.svelte` `handleEvent`, `disconnected` branch.

A `disconnected` event only flips `connected = false` and sets the status text; `series`, `latest`,
and the chart are untouched, so the frozen traces remain on screen with no end marker, no "stopped
at" indication. Combined with P1 from the previous report (any transient read error disconnects),
a technician sees a plausible-looking frozen chart plus a small status change. This is the same
theme as the earlier "dead feed" finding, now with a concrete trigger: the adapter error path.

**Suggested fix:** surface the disconnect cause next to the graph (and in the status area), and
visually mark the live chart as ended/frozen (e.g., dim it, add an end-cap line at the last real
sample).

## Secondary observations (no action required)

- **J2534 worker assumptions that hardware validation must confirm** (this path is unvalidated per
  `PROJECT_STATUS.md`): the driver returns ISO-TP-*reassembled* payloads (no PCI byte) with a 4-byte
  CAN-ID prefix; `PassThruReadVersion` reports exactly `04.04`; the provider accepts 8
  `FLOW_CONTROL_FILTER` registrations; 250 ms read window. Each is a plausible-but-unverified
  contract with real providers — worth logging during the first physical session.
- `update_recording_marker` returns early leaving the transaction to roll back on drop — correct,
  just noting it was checked.
- The automatic-trigger detector intentionally keeps the hold candidate alive inside the hysteresis
  band (pinned by tests); not a bug.
- `SampleQuality::Stale` still has no producer — see the earlier reports for the liveness theme.

## Method and cleanup

- All source views via `git show`/`git grep`; the benchmark test
  (`crates/lognook-core/tests/zz_audit_perf.rs`) was created, run, and deleted — `git status`
  shows only the two untracked report files as changes from this thread.
- Reproduce H2 with a synthetic recording if desired; a permanent `#[ignore]`-style perf harness
  would be a reasonable follow-up.
