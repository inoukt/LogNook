# Communications Checks

## Goal

Give an operator short, repeatable in-app checks for a live OBD adapter and the RH-02/Candlelight CAN adapter. Use the existing serialized owners, acquisition diagnostics, and CAN evidence capture; do not add another transport path.

## OBD link check

Settings → Diagnostics offers a 10-second check while a live OBD adapter is connected. It observes the acquisition polls already running through the common adapter owner, so it adds no requests and does not interrupt recording. It reports poll attempts, successful and failed polls, median/P95 poll duration, and per-signal attempt/success/error deltas when diagnostic snapshots bracket the check. The check retains its own bounded set of relevant events so unrelated PID-read detail cannot evict the measured interval from the general diagnostics view. The simulator can exercise the same summary path before hardware is available.

The result is healthy only when polls succeed and every selected PID reported as supported has delivered a sample without attributable errors. Successful empty polls and silent PID reads are issues, not a pass.

For an OpenPort2 hardware run, use the existing provider refresh, Identify, Connect, and selected standard Mode 01 signals first. Then run the timed check and export the support bundle. Keep connect-stage failures, PID-level errors, and grouped poll failures distinguishable. Do not retune timeouts until repeated real-adapter measurements support a change.

## RH-02/Candlelight one-shot check

The CAN Workbench adds **Try CAN once**. It sends one Mode 01 PID 00 request (`01 00`) using the operator-configured request/response IDs and ID width. It is available only with the adapter connected in active mode and a paired raw capture open. The existing serialized Candlelight scan owner records the transmitted request, ECU response or timeout, typed outcome, and exact route in the capture/evidence sidecar. It does not start on connect or retry automatically.

The request/response route is operator-controlled; displayed defaults are not a universal vehicle route. A matching positive response closes the request/reply path. No response remains inconclusive until adapter state, bitrate, route, and vehicle bus are confirmed. The paired capture retains the evidence for review.

## Verification and first hardware runs

- UI helper tests filter OBD polls by adapter and time window, calculate latency percentiles and per-signal counter deltas, and distinguish empty intervals from failures.
- CAN helper tests prove the quick action creates exactly one PID 00 request with the selected route and rejects invalid IDs.
- OBD first run: on the laptop, connect OpenPort2 using normal app controls, select supported standard PIDs, run the 10-second check, and export the support bundle. Review provider/Identify/Connect stages, poll latency, per-PID delivery, and errors.
- RH-02 first run: select the verified bitrate and route, connect active, start the paired capture, click **Try CAN once**, then inspect its TX/RX result and save/close the capture. Verify the USB adapter and bus separately if no response arrives.
- Physical results are pending; software tests do not establish OpenPort2 provider compatibility or RH-02 vehicle-route compatibility.

## Boundaries

The OBD check measures existing acquisition traffic; it does not probe new PIDs. The CAN check is a single standard read request. Neither changes the existing adapter timeout, normal acquisition policy, catalog validation state, or raw CAN transmission behavior.
