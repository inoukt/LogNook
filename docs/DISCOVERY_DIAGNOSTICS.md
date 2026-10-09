# CAN discovery and vehicle diagnostics

## CAN Help and control explanations

Open the **CAN** tab and select the small **?** beside **CAN bus fields**.
The Help guide explains connection settings, raw capture, the Counts/Frames/Events
views, repeated event marks, field decoding, trigger windows, scans, manual
frames, and saved-capture analysis.

The guide's **Show control tips** checkbox enables or disables the short hover
explanations throughout the CAN controls and field editor. LogNook remembers
the choice locally after restart. These hints use the existing compact native
tooltip style and take no permanent workspace space. Opening Help or changing
the checkbox does not send an adapter request.

## Find an unknown CAN signal

1. Open the **CAN** picker tab and connect RH02/Candlelight at the bus bitrate.
2. Start a paired raw capture. Give the vehicle event a short label, such as
   `traction switch`, and set its before/after observation windows.
3. Perform the physical action and mark it. Repeat the same labeled action to
   distinguish recurring changes from unrelated traffic.
4. In **Traffic monitor → Events**, inspect the ranked candidates. The table
   shows observed trials, before/after traffic, byte transitions and noise hints.
5. Click **Scope B…**, a changed **B…bit** button, or **Inspect → graph**.
   The field editor opens with that byte/bit selected and the transition evidence.
6. Keep **Follow live RX** enabled to see incoming frames and a small scope;
   disable it to inspect the original candidate frame. Set length, byte order,
   signedness, factor, offset and unit. The decoded preview uses the Rust decoder.
7. Select **Save and graph**. The saved field uses the existing graph, recording,
   CSV and replay pipeline. Its selection persists with workspace preferences.

The scope contains at most 120 matching frames from the recent traffic buffer.
Raw capture retains the full evidence. Ranking suggests where to investigate;
it does not establish what the signal means. Counter/checksum hints are also
observations, not proof.

## Read vehicle diagnostics

Open **Vehicle diagnostics** from the top bar after connecting OBDLink SX,
Bluetooth ELM327, or A30. Select **Read diagnostics** to request the diagnostic
snapshot. Opening the view alone does not issue requests.

- **DTCs:** stored, pending and permanent code categories, separated by ECU.
- **Identity:** VIN, calibration identifiers and ECU name when returned.
- **Mode 06:** supported monitors and selected test results. Unmapped unit/scaling
  identifiers remain raw values, with their raw minimum and maximum.
- **Raw:** request/reply bytes, timestamps, duration and typed outcomes.

Unsupported, negative, pending and malformed replies remain visible. An empty
code list is meaningful only for a successful reply. Results retained from a
previous adapter are identified as historical. Support ZIP export includes the
saved diagnostic evidence. OpenPort and the simulator currently have no
dedicated diagnostic-read implementation.

## Run one quick OBD mode test

Connect **OBDLink SX** or **A30**, open **OEM**, select an **OBD read mode**, and
press **Try once**. Both adapters offer the same list:

- Mode 01 engine RPM and Mode 02 freeze-frame RPM (frame `00`).
- Modes 03, 07 and 0A for stored, pending and permanent DTCs.
- Mode 06 bitmap pages `00`, `20`, `40`, `60`, `80`, `A0`, and the Scion-reference
  monitor tests `35`, `A2`, `A5`.
- Mode 09 VIN.

The SX path uses its connected CAN 11/500 session and a functional `7DF` request;
the A30 path retains its captured transport commands. A test does not reconnect
the adapter or start a scan. The existing adapter owner serializes it with PID
acquisition. Replies retain exact commands, raw bytes, ECU, timing and outcomes
in **Research run results**. Drag the small divider above the results to resize
the panel; its handle also accepts the arrow keys.

Decoded RPM and Mode 06 numeric results enter the shared graph and recording
pipeline. Unmapped Mode 06 results use **raw count**. The Scion-reference test
names do not establish that another vehicle supports them. Raw replies from
multiple responding ECUs remain available; this quick-test result format does
not combine their values into one graph channel.

A30 evidence remains in `oem-validation/a30-read-tests.jsonl`; SX quick-test
evidence is in `oem-validation/obd-read-tests.jsonl`. Both are included in the
support ZIP. **Export run report** saves the displayed run and its raw replies.

## Record OBD and CAN together

Connect one OBD adapter and one RH02/Candlelight adapter before starting a
recording, in either order. Each adapter retains its own controls and command
owner. Adding an adapter during recording requires stopping that recording.

Decoded CAN fields and OBD measurements share the session's elapsed timeline;
each sample retains its source and ECU identity. The raw CAN capture also retains
its own CSV/JSONL evidence. The top OBD disconnect and the CAN disconnect operate
independently; a connected peer keeps acquiring.

For an agent, use `get_status`, `read_vehicle_diagnostics`, and
`disconnect_source`; setup and examples are in [Agent Access](AGENT_ACCESS.md).
