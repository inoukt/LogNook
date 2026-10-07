# Vehicle Test Workbench Design

## Purpose

Combine the A30 adapter checks and source-backed OEM candidate testing into one focused workspace. The workbench should make a real adapter test easy to run, keep the graph visible, and preserve enough evidence to understand both successful and failed requests.

## Approved product behavior

- Use the **Live-first** layout selected in the visual comparison: adapter and quick-test controls plus OEM search on the left; the existing graph and recent results on the right.
- Keep the connected adapter and identity visible. Reuse the current paired-device refresh, Identify, Connect, and acquisition controls.
- Offer quick checks for the modes captured from A30: Mode 01 RPM, Mode 02 freeze-frame RPM, Mode 03 stored DTCs, Mode 06 bitmap pages and Scion TIDs 35/A2/A5, Mode 07 pending DTCs, Mode 09 VIN, and Mode 0A permanent DTCs. Only display checks supported by the selected adapter.
- Let the user check source-backed OEM candidates, search and change pages, and retain selections across those changes. Show a compact selected count and run only the explicitly selected items.
- Run the selected queue sequentially. Show queued/running/completed state and a per-request result. Stop finishes the active read and skips later requests.
- Continue after negative, no-response, wrong-ECU, or malformed results while the adapter remains usable. Stop the queue on transport loss. Retries are manual per result.
- Keep numeric results on the shared graph sample pipeline. When recording is active, store those samples in the active recording so replay and CSV export retain them. Preserve nonnumeric results as diagnostics.
- Preserve exact request and response evidence separately from the read-only catalog and offer an exportable run report.

## Layout

Add a compact **Test Workbench** mode switch to the existing Acquisition workspace. Switching modes reuses the same left/right layout and graph; it does not create a second graph or separate acquisition window. The left column contains:

1. A compact adapter/status strip with refresh, identify, connect, and current identity.
2. A quick-check section with the selected adapter's available one-shot tests.
3. The existing OEM research search and filters, with per-candidate selection checkboxes.
4. A compact selected count and **Run selected** / **Stop** controls.

The right column keeps the current graph at its normal large size. A compact, independently scrollable results table sits below it and can be collapsed or resized. Each result row includes test/candidate, status, latency, responder, and decoded values; expanding a row shows raw request/response and source details. The existing chart preferences continue to control signal color and line style.

## Request ownership and scheduling

All vehicle requests go through the existing acquisition adapter owner. The workbench must not open a second serial/RFCOMM connection or start a competing worker while acquisition is connected. Add a typed research/test request to the existing request path for the adapters that support it:

- OBDLink SX: exact catalog-backed service 21/22 candidate reads.
- Bluetooth ELM327: standard Mode 01 acquisition and exact catalog-backed service 21/22 candidate reads.
- Xtool A30/A30M: standard Mode 01 acquisition, the captured one-shot modes above, and exact catalog-backed service 21/22 candidate reads.
- OpenPort 2: keep OEM queue actions unavailable until a read-only request path is implemented and tested for that adapter.

The acquisition owner gives due configured logging polls priority. It runs a queued research read between polling cycles when safe, then returns to logging. Research work may increase observed poll latency while a request is in flight; it must not discard samples already acquired. Record request latency and observed live sample rate rather than assuming the research request cost.

Catalog candidates are reloaded and validated on the backend before transmission. The request must retain the catalog's service, identifier width, ECU route, source revision, license, and decoder. No numeric identifier sweep, arbitrary raw command, state change, security access, session change, routine, or actuator action is available.

## Results, graph, and evidence

Each run has an ID, adapter identity, start time, and ordered request results. Each result records the candidate or quick-test ID, exact request and route, start time, latency, responding ECU, typed outcome/NRC, raw response, decoded channels, decoder/source provenance, and unchanged research/vehicle-validation state.

Finite numeric decoded values publish as `Sample` events through the existing engine pipeline. Catalog values outside the catalog range remain graphable with `Suspect` quality. Unmapped Mode 06 numeric values remain `raw count`; DTC and VIN text do not create fabricated numeric signals.

Detailed evidence remains outside the read-only SQLite catalog in application data. Existing one-shot evidence records remain supported; queued runs add run identity and item order to their evidence. The UI provides an exportable JSON run report. If recording is active, only actual numeric sample events enter the `.lognook` recording; report details remain in the research evidence file.

## Failure and stop behavior

- A row records every response outcome, including positive, negative/NRC, response-pending, temporarily unavailable, no response, malformed/wrong length, wrong responder, and disconnect.
- Ordinary per-request outcomes do not silently become vehicle support or decoder validation.
- A recoverable ECU outcome advances to the next selected item. A lost adapter stops the run and leaves unstarted items available to run later.
- Stop is cooperative: the in-flight request completes or times out; no later item is sent.
- A user retry is a new recorded attempt. There is no automatic retry loop.

## Testing and acceptance

- Rust tests cover queue order, one active request owner, scheduled-poll priority, outcome continuation, cooperative stop, link-loss handling, evidence provenance, and graph/recording delivery of numeric results.
- Adapter tests prove exact candidate requests and route checks, A30 captured request bytes, and rejection of requests absent from the supported test definitions.
- UI tests cover adapter-specific quick tests, candidate selection across search/page changes, selected count, queue progress, stop, per-result diagnostics, export, and graph updates.
- On a physical adapter, acceptance begins with Identify, a Mode 01 RPM check, then one chosen source-backed vehicle candidate. Capture request, response, ECU route, latency, and observed sample rate. A positive read is evidence of a response, not automatic vehicle validation.

## Out of scope

Automatic candidate sweeps, arbitrary request entry, writes/clears/routines/actuators, automatic decoder promotion, new vehicle-profile matching, and OpenPort OEM requests are excluded from this workbench design.
