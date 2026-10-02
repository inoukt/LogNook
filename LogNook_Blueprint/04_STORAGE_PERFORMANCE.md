# Storage, scheduler, analysis and performance

## Source-of-truth format
One `.lognook` SQLite file per session, WAL journaling while recording; finalized checkpoint and integrity check on clean close. Minimum entities: `session`, `source`, `ecu`, `signal_definition`, `sample_chunk` or `sample`, `raw_frame_chunk`, `event`, `marker`, `trigger`, `derived_definition`, `workspace_snapshot`, `provenance`, `schema_version`. Use canonical units and retain original decoded payload/raw bytes where available. If sustained raw-CAN throughput exposes SQLite row overhead, store bounded compressed time chunks as blobs while retaining a queryable index and exact timestamp order. Don't preemptively build a custom database.

## Timestamp and quality
Use monotonic high-resolution receive timestamps for relative ordering, separate wall-clock UTC origin for human labels. Preserve hardware timestamps where available with source and clock-quality metadata. Do not treat timestamps across separate USB devices as perfectly synchronized. Signal states: valid, stale, missing, timeout, parse-error, disconnected, suspect. Keep actual sample counts and age. Each reconnect inserts an explicit event; graphs break lines by default at unknown gaps.

## Polling algorithm
1. Capability discovery identifies supported PIDs/DIDs and response sources without assuming unsupported data is zero.
2. Assign per-signal requested rate, minimum acceptable rate, priority, response deadline and estimated ECU/bus cost. Keep per-ECU queues and exclusive per-device command ownership.
3. Batch compatible CAN Mode 01 PIDs only if ECU and adapter validation confirms correctness; grouping is not universal and can increase latency on incompatible devices.
4. Separate UDS requests by ECU/session and obey measured response-pending timing; do not saturate a gateway.
5. Continuously measure request-to-response latency, achieved Hz, timeout/error ratio and observed bus load where measurable. Adapt with additive gains/conservative backoff, fairness and user-configurable bus-load caps.
6. Timestamp samples on receipt, never assign an arbitrary fixed periodic timestamp. UI renders decimated snapshots while ingest persists full resolution.
7. On failure stop the affected flow, publish a degradation reason, attempt bounded exponential reconnect, verify session and resume; never fill gaps with the last value without labeling it held/stale.

## UI graph streaming
A transport-neutral `TimeSeriesProvider` returns viewport-limited samples and aggregates. Preserve per-pixel min/max envelopes so transient spikes survive decimation; support exact zoom on stored data and specialized XY density rendering for scatter. Update visible charts at a capped ~20–30 frames/s target independent of acquisition frequency; measure actual performance with large 1 h/8 h logs and low-power technician laptops. Renderers: uPlot for synchronized time series; Apache ECharts for scatter, heatmaps, bar/histogram. Shared chart state, shared cursor and common signal styling.

## CSV import/export
- **Native long format** for lossless asynchronous samples: `timestamp_iso,elapsed_ns,source_id,ecu_id,signal_key,value,unit,quality` plus metadata/marker sidecars. User may choose raw bytes export.
- **Wide aligned CSV** for conventional tools/Datazap: first column elapsed seconds, one column per selected signal, with explicitly selectable bin interval and alignment policy (`blank`, `last-known with age limit`, or interpolation when physically appropriate). Never silently interpolate discontinuities. Preserve canonical names/units and document transformations.
- Robust import wizard: encoding, delimiters, decimal symbols, UTC/timezone, units, duplicate timestamps, metadata columns and user mapping. Preview changes before import, store original file checksum, allow repeatable mapping presets.
- Screenshots export selected range/annotations with optional VIN/job masking. Sharing jobs run in background with retries and do not block acquisition.

## Derived values and known-good
Sandboxed formula grammar with documented functions and dimensional validation; no `eval`, script plugins or recursive infinite dependencies. Conversions/tare are view transformations and versioned derived signals. Comparison stores test procedure/context and offers time/event/RPM alignment with explicit uncertainty; calculate differences only over comparable windows.

## Provisional measurable targets (to be validated)
UI opens to last workspace without requiring vehicle connection; graph interaction remains responsive while acquisition and CSV export are running; physical adapter replug preserves existing recording and shows lost-data interval; no silent sample loss during bounded overload tests. Performance targets are acceptance goals rather than promised real-car frequencies.
