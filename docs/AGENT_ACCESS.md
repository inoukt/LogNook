# Agent Access

Agent Access lets a local MCP client inspect and operate the running LogNook app. Adapter I/O stays inside LogNook's existing owners; the MCP process only sends typed requests over a same-user local socket.

## Set up

1. Open **Settings → Agent Access** in LogNook.
2. Set a six-digit PIN. Keep LogNook open.
3. Select **Copy agent setup** and add the copied `mcpServers.lognook` entry to the MCP host on this same PC.
4. Unlock Agent Access in Settings before starting or reconnecting the MCP host.
5. Ask the agent to call `get_status` first, then `list_adapters` or `list_log_sources` and the specific tool it needs.

The copied configuration points at the current LogNook executable and its temporary local endpoint. It contains no PIN. The Codex-launched MCP sidecar monitors that local connection and exits within about one second after LogNook locks or closes. Reopen and unlock LogNook, then reconnect the MCP host before using its tools again.

## Available tools

- `search_subaru_catalog` and `get_subaru_catalog_detail` expose the same local
  Subaru OEM catalog as the UI. Search filters by name/DID, model/year, ECU and
  protocol with bounded pages; details load decoder/source-row provenance on
  demand. `oem_logging` has separate `start_subaru` and `stop_subaru` operations
  for explicit graph logging of candidate IDs returned by that catalog. FHI-CAN
  reads use their exact CAN route. SSM-KWP reads use the `open_port2_subaru_ssm`
  connect choice, then fixed read-only SSM2 init/address-read operations through
  OpenPort 2 K-Line mode. Both modes require the current adapter ID and connection
  generation from `get_status`, retain raw bytes/catalog SHA, and leave candidates
  unverified and `auto_request=0`. See
  [Subaru catalog](SUBARU_CATALOG.md).

- `get_status`, `list_adapters`, and `list_signals` inspect connections and the standard PID set. Status reports OBD and CAN connection state separately, including each adapter identity, the current OBD connection generation, and per-operation diagnostic capability flags.
- `can_obd` provides typed `discover`, `identify`, `connect`, `select`, `status`, `disconnect`, and `cancel` operations for normal Candlelight GS-USB OBD logging. Identify reads USB identity only and sends no CAN frame. Connect uses 11-bit CAN at 500 kbit/s, discovers generic Mode 01 support pages, rejects multiple ECU responders, and feeds selected supported PIDs into the normal live sample/recording pipeline. It does not scan OEM identifiers or run VIN/profile recognition. Use `cancel` while connect is in progress; it does not wait for the engine mutex.
- `list_log_sources` lists bounded live PID/CAN buffers plus saved app recordings and agent CAN captures. `query_log_range` reads an elapsed-time range in milliseconds with optional PID/CAN-ID filters; responses cap at 1,000 rows and report truncation. A known external `.lognook` or `.candump.csv` path can also be queried.
- `connect_adapter` uses the existing owners. OpenPort choices include normal OBD-II CAN and a dedicated `open_port2_subaru_ssm` mode for Subaru SSM K-Line. The latter does not start standard OBD polling or diagnostics. `disconnect_source` with `source: "obd"` or `"can"` disconnects only that source; `disconnect_adapter` disconnects both. Check `get_status` after a partial disconnect. A passive CAN Workbench connection does not transmit; CAN OBD connect performs generic PID discovery. Normal OBD logging and CAN Workbench cannot own the same physical USB interface/channel at once; different channels or devices have separate leases.
- `set_signal_selection` updates logged and graphed PIDs; the OBD poll selection cannot change while OBD is connected. CAN field graph selection remains independent.
- `search_oem_candidates` and `try_oem_candidate` find catalog-backed candidates and run one selected request. Results retain route/source and response evidence; they do not validate a decoder or vehicle support.
- `oem_logging` exposes continuous logging through typed `status`, `search`,
  `start` and `stop` operations. Search takes catalog `vehicle_id`, query, offset
  and limit, and returns only eligible definitions. Start takes that vehicle ID,
  candidate IDs, and the current `adapter_id`/`connection_generation`; Stop is
  bound to the same connection. The UI and MCP use the same source-revision,
  physical validation and scheduler checks. SX, ELM327, A30, OpenPort and
  Candlelight OBD use their existing owners. Raw request bytes and validation
  overrides are unavailable. The current catalog has no validated automatic
  definitions, so search can legitimately return no eligible entries. Stopping
  removes the OEM plan while ordinary OBD logging continues. As with normal
  acquisition, an explicitly started plan continues until Stop/disconnect;
  locking Agent Access prevents new MCP commands.
- `oem_discovery` exposes the OEM list's shared one-shot queue. Its typed `operation`
  supports `status`, `search`, `set_auto_try`, `enqueue_visible`, `try_filtered`
  and `stop`. Mutating starts/enqueues require the current `adapter_id` and
  `connection_generation` from `get_status`. `status.candidate_ids` returns only
  the requested bounded set of row results; `search` accepts the catalog filter,
  offset, limit and `values_first`. `try_filtered` traverses all matching pages.
  Each candidate/source revision runs at most once per connection. Finite numeric
  values, including zero, sort first without promoting source validation.
  Stop cancels remaining work; an in-flight bounded exchange may finish. Agent
  Access locking or expiry stops agent-owned background discovery. SX, ELM327,
  A30, OpenPort 2, Candlelight OBD and simulator use their existing serialized
  owner. No raw request bytes, arbitrary identifiers, session changes or source
  promotion are accepted. Replies and numeric values remain separate outcomes,
  and simulator evidence remains labeled simulated.
- `list_mode06_tests` and `try_mode06_test` expose the same nine quick Mode 06 reads on connected OBDLink SX and A30 adapters: bitmap pages `00`, `20`, `40`, `60`, `80`, `A0` and Scion-reference tests `35`, `A2`, `A5`. Support here means the adapter can issue the test; the ECU may return a negative reply or no data. Raw results and finite `raw count` channels use the existing evidence and measurement pipeline.
- `read_vehicle_diagnostics` is read-only. It requests the DTC, identity, and Mode 06 categories the current adapter advertises: SX, ELM327, A30, OpenPort 2, Candlelight OBD, and simulator. OpenPort now supports Mode 06 through the same bounded standard read path. Candlelight reports the single ECU used by its current connection flow and does not merge responders. Optional `mode06_test_ids` adds byte-sized monitor IDs, for example `[53, 162]`; `[]` requests the standard snapshot. Results retain ECU, timestamp, exact request, raw reply, typed outcome and decoded values; unsupported categories are marked not sent. Evidence is saved to `diagnostics/vehicle-reads.jsonl` and included in support ZIP exports. This action never clears codes.
- `clear_vehicle_dtcs` is the separate, explicit standard Mode 04 action. First call `get_status`, then pass its current `adapter_id`, `obd_connection_generation`, and `confirm: true`. It requires unlocked Agent Access and the adapter's `clear_dtcs` capability; stale adapter/generation values are rejected before I/O. It sends one fixed `[04]` request through the same serialized owner, without retries. Raw commands cannot carry Mode 04. Each ECU's exact response and typed outcome is saved as a separate append-only clear report; positive `44` is only an ECU acknowledgement. Clearing may erase emission-related diagnostic information and affect readiness. Read again separately to inspect current DTCs.
- `get_trace`, `start_trace_capture`, `stop_trace_capture`, and `export_evidence` inspect or save the adapter bytes LogNook can observe. The trace reports dropped/evicted-event gaps. J2534 traces cover the worker protocol and returned payloads, not hidden vendor USB traffic.
- `workbench` captures CAN, marks a labeled event, starts/stops configured CAN scans, sends a bounded CAN frame pattern, and replays one captured sequence for a selected CAN ID. For replay, inspect `live:can` with `list_log_sources` and `query_log_range`, then pass `operation.kind: "replay_can_id"`, the exact `can_id`, `extended` format, and `start_elapsed_ms` / `end_elapsed_ms`. LogNook selects only matching unflagged classic RX frames from the current connection and limits replay to the latest 400 frames; saved files and caller-supplied frame data are not replay sources. Replay preserves source intervals with a 1 ms minimum and rejects gaps above 60 seconds. Scan, send, and replay create paired `.candump.csv` and `.jsonl` evidence under app data. Use an active-mode Candlelight connection for scan/TX; connection defaults to listen-only.
- `send_raw_command` sends one diagnostic hex payload or adapter-terminal command through a connected OBDLink SX or ELM327's serialized owner. Mode 04 is rejected and points to `clear_vehicle_dtcs`. Exact TX/RX is available in `get_trace`. Use `workbench` for raw Candlelight CAN frames; unsupported adapter/mode combinations return an error.
- `stop_operation` cancels agent-owned OEM discovery and the current agent CAN scan/send/replay/capture, and finalizes paired evidence. Discovery's current bounded exchange may finish; remaining requests are cancelled. Disconnecting CAN or the adapter also cancels an active replay.

## MCP parity for new features

MCP parity is a required part of feature development. Every new capability with meaningful data or operations must be added to Agent Access/MCP in the same change, using the existing typed IPC request, core service/owner, and authorization and safety gates. Update the MCP tool schema and descriptions, and test that clients can discover and invoke the operation and receive its result/status; cover authorization, cancellation, and evidence behavior where applicable. Do not implement a second MCP-only business-logic path or bypass the desktop app's serialized hardware owner. For a genuinely presentation-only capability with no meaningful MCP equivalent, explain the UI-only decision in `PROJECT_STATUS.md`.

## Access and evidence

Lock with **Lock agent** in Settings. Access also expires after 15 minutes without agent activity. Lock/expiry stops agent-owned CAN scans, frame sends, replays, and captures and finalizes partial evidence; the paired CAN journal records why an operation stopped. Use `stop_operation` to stop earlier. The app shows the current operation and trace gap count in Settings.

The PIN is an in-app access gate. It does not isolate the MCP process from other software already running as the same Windows user. Treat raw protocol replies and CAN frames as research evidence; a response alone does not establish a signal meaning or vehicle validation.

## Example task

> Check `get_status`, connect the selected Candlelight adapter in active mode, start a CAN capture, then run the exact Mode 21 range and route I provide. Stop the scan, export the evidence, and report the request range, route, positive/negative/no-response counts, trace gaps, and the saved CSV/JSONL paths. Do not infer signal meanings from raw replies.
# Signal preset tool

Agent Access exposes `signal_presets` using the same typed operation and result
union as the desktop picker. List/get/suggest and preview are non-transmitting;
Apply requires the opaque preview ID, context revision and connection
generation, and rechecks eligibility at the acquisition owner. Agent lock or
expiry cancels pending preset work and invalidates its preview. Saving creates
or revises a custom recipe and never promotes an OEM source definition.

## VIN vehicle projects

`vehicle_projects` exposes the same project storage and connection-bound choices
as the desktop UI. Use `status`, `list` and `get` to inspect state;
`prepare_on_connect` uses fresh observed VIN evidence. A returning project
requires `load_last_state` or `start_fresh` before `save` is accepted. Mutations
check connection generation and saved revision. `select_manual` is an explicit
association, not ECU identity evidence. Agent Access must be unlocked. Workspace
restore never starts recording, OEM tests/polling or CAN jobs. See
[VEHICLE_PROJECTS.md](VEHICLE_PROJECTS.md) for the UI flow and saved fields.
