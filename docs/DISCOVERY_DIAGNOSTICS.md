# CAN discovery and vehicle diagnostics

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
