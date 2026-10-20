# LogNook current project gap review — 2026-10-15

Scope: static review of the current local working tree at base commit
`eda608b`, branch `codex/can-event-freeze-frame`. This review includes the
uncommitted diagnostics, OEM discovery and scatter changes. It is not a fresh
hardware validation or a comparison with remote GitHub branches. No application
source or catalog records were modified and no test suites were run for this
review.

## Highest-value missing work

Later the same day, the adapter parity item below was implemented and verified
with software fixtures: OpenPort Mode 06, continuous validated OEM transport
hooks for OpenPort/Candlelight OBD, and typed MCP OEM logging controls. See
[the completion report](adapter-parity-2026-10-15.md). The table preserves the
original review snapshot; physical acceptance remains pending.

| Priority | Gap | What exists and what is still needed |
| --- | --- | --- |
| 1 | Reproducible development checkpoint | Recent working features span many modified/untracked files. Review and commit the intended changes, capture checks for that exact tree, and associate release artifacts with that revision. No `.github/workflows` directory was found in this checkout. |
| 1 | OEM vehicle validation workflow | The shipped catalog contains 37,093 OEM candidates, all `auto_request=0` and `vehicle_validation=not_tested`. It contains zero validation sessions/results. One-shot responses are useful evidence but do not create an eligible continuous logging definition. Add an explicit evidence review/validation workflow tied to vehicle, ECU route and calibration, preserving source records and the current gates. |
| 1 | Canonical concept search and applicability resolution in the app | The database has 75 concepts, 173 aliases and 13,357 candidate/concept mapping rows covering 11,948 distinct candidates. The app's OEM search uses candidate-field LIKE filters and does not query those concept/alias tables. Integrate concept search and a context-aware definition resolver; 25,145 candidates currently have no concept mapping. |
| 1 | DBC import | The CAN field editor/decoder and an explicitly unverified DBC skeleton export exist. There is no DBC import workflow found in the app/core. Add import preview, source attribution, units/scaling/bit layout, explicit unsupported constructs and deliberate multiplexing handling. |
| 2 | Adapter capability parity | OpenPort standard DTC and Mode 09 identity reads now work; Mode 06 remains explicitly unsupported. OpenPort and Candlelight OBD have OEM one-shot research but no continuous OEM polling implementation. The continuous OEM hook defaults to unsupported for them. |
| 2 | Wider diagnostic protocol/ECU coverage | Raw CAN Workbench supports 11/29-bit frames. The current OpenPort and Candlelight normal OBD paths remain 11-bit CAN at 500 kbit/s with one discovered responder. Broader OBD addressing/bitrates, controlled ECU browsing, SLCAN/SocketCAN, K-line/J1850, Subaru SSM, J1939 and DoIP are not established application capabilities in the reviewed implementation. Add only as a selected, separately validated scope. |
| 2 | Performance and failure acceptance | Add focused regression coverage for asynchronous scatter pairing, stale values, frame-batched delivery, reconnect/reset and session switches. `pendingLiveSamples` has no explicit pending-queue bound/fallback; scatter's third-signal color range scans the full retained signal history despite the point caps. Measure and address those paths, then run sustained simulator/hardware recordings and background/sleep/disconnect/disk-full acceptance. Existing fixture tests cover some failures; they do not establish the full long-duration matrix. |
| 3 | Workshop analysis depth | Graphs, formulas, replay, markers and CAN comparisons exist. General measurement-range statistics/percentiles, histogram charts, synchronized comparison of multiple diagnostic recordings and richer saved graph annotations remain product targets. Raw CAN byte histograms and latency percentiles are already implemented and should not be counted as missing. |
| 3 | Portable workspaces/profiles and richer offline identity | Local profile recognition/Resume and full VIN history exist. No profile/workspace export-import workflow was found. Offline VIN decoding provides WMI assignment, applicable checksum status and model-year candidates; model/trim/engine decoding remains limited and manual details are separate. |
| Release | Distribution and contribution readiness | Tauri bundling is disabled, so the current release artifact is a runnable executable. There is no root LICENSE file, only a policy proposal. Installer/portable packaging, a repeatable worker/catalog layout, clean-PC acceptance and a CI/release workflow still need completion before convenient public distribution. |

## Database measurements

Read-only queries against
`PIDs/LogNook_PID_Master_v0_3/data/lognook_pids.sqlite` returned:

- OEM candidates: **37,093**.
- All candidates: **not_tested**, **auto_request=0**.
- Canonical concepts: **75**; concept aliases: **173**.
- Candidate/concept mappings: **13,357**, covering **11,948** distinct candidates.
- Candidates without a concept mapping: **25,145**.
- Validation sessions: **0**; validation results: **0**.

These counts describe the bundled catalog, not every manual probe/report or the
number of physically compatible signals. Catalog queries used Node's SQLite
reader with `readOnly:true`; no validation state was changed.

## Documentation drift to clean up

- `docs/FEATURE_BACKLOG.md` still lists recording profile context as missing;
  `vehicle_context_v1` recording metadata and its pipeline integration exist.
- The same backlog still calls OpenPort diagnostic reads open work, although
  standard DTC and identity reads have been implemented and the owner supplied
  positive Ford results. Mode 06 remains missing.
- `docs/VEHICLE_PROFILES.md` still says OpenPort's full DTC diagnostics are
  unsupported. Its Mode 06 limitation is still accurate.
- The latest scatter status entry still contains a pending-release sentence
  after recording a completed release build. The owner subsequently reported
  that the simulator display seemed fine; that acceptance is not recorded there.
- Dated blueprints and historical test reports should retain their original
  outcomes; update current summaries rather than rewriting history.

## Evidence locations

- `crates/lognook-core/src/oem_catalog.rs`: text-based catalog search and gated
  continuous OEM selection.
- `crates/lognook-core/src/adapter.rs`: default unsupported continuous OEM hook.
- `crates/lognook-core/src/openport2.rs`: Mode 06 capability false/rejection.
- `crates/lognook-core/src/can_obd.rs`: fixed normal OBD CAN bitrate and responder
  discovery restrictions.
- `crates/lognook-core/src/can_analysis.rs` and
  `apps/desktop/src/CanWorkbenchPanel.svelte`: DBC skeleton export, not import.
- `apps/desktop/src/App.svelte`: pending live sample queue and animation-frame
  flush.
- `apps/desktop/src/ScatterPlot.svelte`: retained-history color range calculation.
- `crates/lognook-core/src/recording.rs` and `pipeline_worker.rs`: recording
  vehicle context already implemented.
- `apps/desktop/src-tauri/tauri.conf.json`: bundling disabled.
- `docs/FEATURE_BACKLOG.md`, `docs/VEHICLE_PROFILES.md`, `PROJECT_STATUS.md`,
  `LogNook_Blueprint/01_PRODUCT.md` and `05_VALIDATION_ROADMAP.md`: current goals,
  implementation status and historical acceptance targets.

Recommended order: establish a checked-in development checkpoint and current
status; finish the focused performance/lifecycle checks; add the OEM validation
and concept-resolution workflow; then choose DBC import as the next substantial
CAN feature. Broader protocols and analysis features should follow measured
needs rather than being added together.
