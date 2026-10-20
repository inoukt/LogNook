# CAN discovery and vehicle diagnostics

## Normal Candlelight OBD logging

Choose **Candlelight OBD (GS-USB)** in the main adapter selector, select the USB
device and interface/channel, then press **Identify**. Identify checks the USB
device descriptors and GS_USB identity only; it does not start CAN or send a
vehicle request. Select the generic OBD signals to log, then press **Connect**.

Connect starts 11-bit CAN at 500 kbit/s, requests the standard Mode 01 support
bitmap, follows advertised bitmap continuation pages, and requires one unique
standard OBD responder. It exposes only signals with existing generic Mode 01
decoders in the ordinary PID picker. The selected channels use the same live
graphs, Record, CSV, session and replay pipeline as other OBD adapters. OEM
Numeric Mode 21/22 scans and VIN/profile recognition are not part of the normal
connection flow. Explicit source-backed OEM one-shot research is available in
the OEM catalog, as described below.

One LogNook process owns each physical USB interface/channel. If CAN Workbench
already owns the selected channel, Identify or Connect returns an ownership
error; disconnect that source before switching. All-empty cycles wait briefly
before retrying so unsupported or absent replies do not become a tight poll
loop. Successful and partial cycles remain paced by ECU response time.

## Try OEM catalog results once

In the **OEM** list, enable **Auto-try visible rows once per connection** to try source-backed candidates
as their rows enter the visible scroll area. The checkbox starts off and resets
after disconnect/reconnect. Each candidate and source revision is attempted at
most once during that connection, including rows revisited while scrolling.

**Try all filtered** walks every result matching the current search and filters,
including later pages. Returned finite numeric values move to the top across
pages; zero is a value. Sorting does not compare unrelated units or change the
catalog's order within the value/non-value groups. **Stop** cancels queued work;
an exchange already in progress can finish within its existing timeout.

Rows use both text and color: green for a decoded numeric value, amber for a
reply without a decoded number, red for a negative reply, timeout or error, and
blue for queued/running work. A returned number is not proof of decoder accuracy
or vehicle compatibility. Inspect the source, route, raw response and outcome.

The same serialized research service is available through Agent Access/MCP and
supports SX, ELM327, A30, OpenPort 2, Candlelight OBD and the simulator. It uses
the existing connected adapter; a passive CAN Workbench connection is a separate
mode. Only catalog-backed read services 21/22 with complete provenance and exact
supported addressing can run. Simulator results are explicitly simulated.
Discovery preserves append-only try evidence and does not select graph channels,
enable continuous logging, change source validation, or add candidates to presets.
Continuous OEM logging is also available through OpenPort and Candlelight OBD,
but only for explicitly selected, physically validated catalog definitions for
the exact vehicle. Discovery responses never make a candidate eligible.

## CAN Help and control explanations

Open the **CAN** tab and select the small **?** beside **CAN bus fields**.
The Help guide explains connection settings, raw capture, the Counts/Frames/Events
views, repeated event marks, field decoding, trigger windows, scans, manual
frames, selected-ID replay, and saved-capture analysis.

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

### Compare repeated-event snapshots

In **Traffic monitor → Events**, mark the same labeled physical action more than
once using the configured before/after windows. When a group has marks, choose
**Compare snapshots** to open its resizable comparison. Each chronological
column is one mark; each row is a CAN ID plus its standard/extended width. Cells
show the latest RX payload and the number of frames seen for that ID in the
window. Use the ID filter and horizontal/vertical scrolling to inspect larger
groups. Opening or closing the dialog does not stop capture or send bus traffic.

- **Green:** exact payload recurrence among known completed observations. Stronger
  green indicates greater recurrence. At least two known observations are
  needed before relation colors appear.
- **Amber:** a different payload for the same CAN ID and standard/extended width,
  with the same DLC, matches at least 90% of the bits in the most common exact
  payload for that same ID and width.
- **Neutral:** an observed payload is not an exact recurrence or does not meet
  the same-DLC similarity threshold.
- **Absent:** the ID was not observed in a complete window whose ID list was not
  capped.
- **Unknown / not retained:** the window omitted IDs because its 100-ID limit
  was reached, so absence cannot be concluded. Pending and incomplete marks
  are separately labeled and do not count toward recurrence.

The view compares only marks with the same label and keeps standard and extended
IDs separate. The compact live summaries retain up to 100 IDs per completed
window; the raw capture remains the complete frame evidence. Clearing live event
analysis clears the summaries together with the live ranking but preserves the
saved raw capture and event marks. Reopening a saved capture does not rebuild
this comparison. Colors show byte-pattern correlation only; correlation does
not decode signal meaning or identify counters, checksums, ECUs, or vehicle
functions.

### Replay one selected CAN ID

In the live **Traffic monitor → Counts** or **Frames** view, select an exact
standard or extended CAN ID to freeze its matching retained RX data frames.
Review the snapshot first; selecting an ID never transmits. Choose **Start TX
replay · once** only when you intend to send that captured sequence. Replay
requires an active-mode CAN connection and a paired capture, preserves the
observed intervals with a 1 ms minimum, and rejects gaps over 60 seconds. It
uses at most the latest 400 retained frames. The live CAN journal records the
source timing and transmitted-frame results; the displayed frame does not
establish that retransmission is harmless or that an ECU accepted it. Keep the
vehicle stationary and use the persistent **Stop replay** control if the
sequence should end early. Disconnecting the CAN adapter also stops replay.

Agent MCP exposes live replay after Agent Access is unlocked. It takes
the CAN ID, frame format and live elapsed-time range, then resolves the frames
inside LogNook from the current CAN connection; saved captures and MCP-supplied
payloads are not inputs to that live operation. The MCP steps and schema are documented in
[Agent Access](AGENT_ACCESS.md).

### Transmit a selected ID from a saved capture

Open a LogNook `.candump.csv` in saved-capture analysis. In its TX replay panel,
choose one exact ID/11- or 29-bit format and an inclusive source elapsed-time
range in milliseconds, then **Prepare TX replay**. Review the immutable preview,
source SHA-256, source row/time range, RX count, excluded TX/flagged rows and
effective duration. Preparation sends no frames.

Connect the intended Candlelight target in active mode and start a different
paired output capture. **Start TX replay · once** uses the freshly inspected
target generation and the backend-frozen source. Known source/target bitrate
mismatches block Start. Use the persistent Stop control; disconnect/capture
closure/failure cancels remaining frames. Original source files remain unchanged;
source frames/provenance and new TX results are journaled separately.

Saved sources are limited to 256 MiB and clips to 1–10,000 valid unflagged classic
RX frames, with source and paced durations each at most 600 seconds. Gaps over
60 seconds, malformed source rows and out-of-order matching timestamps reject
preparation. Over-limit clips must be narrowed; they are never silently truncated.
The existing 1 ms interval minimum/no catch-up and one-pass behavior apply.

MCP uses a listed or UI-registered saved CAN source, then Workbench operations
`prepare_saved_can_replay`, `inspect_saved_can_replay` and explicit
`start_saved_can_replay` with its preview ID and expected CAN generation.
`discard_saved_can_replay`/`stop_operation` cancel preparation; Agent lock revokes
pending work. Preview tokens expire after 10 minutes or replacement. Numeric CSV
or `.lognook` measurement replay remains a separate on-screen workflow.

### Resume vehicle selections

The top-bar **Profiles** workflow recognizes the vehicle/ECU and offers an explicit
eligible-selection preview. See [Vehicle/ECU profiles](VEHICLE_PROFILES.md) for
first Save, subsequent Connect/Resume, fallback, storage and MCP operations.

## Read vehicle diagnostics

Open **Vehicle diagnostics** from the top bar after connecting OBDLink SX,
Bluetooth ELM327, Xtool A30/A30M, OpenPort 2, Candlelight OBD, or the simulator.
The panel reads each adapter's negotiated diagnostic capabilities. Opening the
view alone does not issue requests.

- **DTCs:** stored, pending and permanent code categories, separated by ECU.
- **Identity:** VIN, calibration identifiers and ECU name when returned.
- **Mode 06:** supported monitors and selected test results. Unmapped unit/scaling
  identifiers remain raw values, with their raw minimum and maximum.
- **Raw:** request/reply bytes, timestamps, duration and typed outcomes.

OpenPort reads standard DTC categories, Mode 09 identity and Mode 06 tests through
its existing serialized J2534 channel. Candlelight reads standard DTC, Mode 09 and Mode 06 data through
the single ECU selected by its existing connection flow; it does not merge
multiple ECUs. The simulator displays clearly labeled local results.

Unsupported, negative, pending and malformed replies remain visible. An empty
code list is meaningful only for a successful reply. Results retained from a
previous adapter are identified as historical. Support ZIP export includes the
saved diagnostic evidence. Unsupported categories are recorded as not sent.

## Explicitly clear DTCs

The separate **Clear DTCs** action sends one standard Mode 04 request through
the connected adapter's serialized owner. It is capability-gated and requires
confirmation that names the connected adapter and responding ECU scope. The
confirmation explains that clearing can erase stored emission-related
diagnostic information and affect readiness state. Cancel sends nothing. On
Candlelight, the current connection is limited to its single discovered ECU.

Each clear operation is saved as its own append-only diagnostic report. A
positive `44` response means the ECU acknowledged the request; it does not prove
that a later DTC read will be empty. Run **Read diagnostics** separately to
check current codes. The app does not automatically retry a clear or follow it
with another read. Simulator clearing is a labeled local result and sends no
vehicle request. Raw commands cannot send Mode 04.

Agent Access exposes the distinct `clear_vehicle_dtcs` MCP action. It requires
unlocked Agent Access, `confirm: true`, and the current adapter ID and OBD
connection generation returned by `get_status`. `read_vehicle_diagnostics`
remains read-only and never clears codes.

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
