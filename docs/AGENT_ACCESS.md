# Agent Access

Agent Access lets a local MCP client inspect and operate the running LogNook app. Adapter I/O stays inside LogNook's existing owners; the MCP process only sends typed requests over a same-user local socket.

## Set up

1. Open **Settings → Agent Access** in LogNook.
2. Set a six-digit PIN. Keep LogNook open.
3. Select **Copy agent setup** and add the copied `mcpServers.lognook` entry to the MCP host on this same PC.
4. Unlock Agent Access in Settings before starting or reconnecting the MCP host.
5. Ask the agent to call `get_status` first, then `list_adapters` and the specific tool it needs.

The copied configuration points at the current LogNook executable and its temporary local endpoint. It contains no PIN. The MCP host must be restarted after the app locks or closes.

## Available tools

- `get_status`, `list_adapters`, and `list_signals` inspect the connected adapter and standard PID set.
- `connect_adapter` and `disconnect_adapter` use LogNook's existing adapter owners. Connection alone does not start a scan or transmit.
- `set_signal_selection` updates logged and graphed PIDs; logged selections cannot change while connected.
- `search_oem_candidates` and `try_oem_candidate` find catalog-backed candidates and run one selected request. Results retain route/source and response evidence; they do not validate a decoder or vehicle support.
- `list_mode06_tests` and `try_mode06_test` expose the supported A30 reads.
- `get_trace`, `start_trace_capture`, `stop_trace_capture`, and `export_evidence` inspect or save the adapter bytes LogNook can observe. The trace reports dropped/evicted-event gaps. J2534 traces cover the worker protocol and returned payloads, not hidden vendor USB traffic.
- `workbench` captures CAN, marks a labeled event, starts/stops configured CAN scans, and sends a bounded CAN frame pattern. A scan or send creates paired `.candump.csv` and `.jsonl` evidence under app data. Use an active-mode Candlelight connection for scan/TX; connection defaults to listen-only.
- `stop_operation` cancels the current agent CAN scan/send/capture and finalizes partial evidence.

## Access and evidence

Lock with **Lock agent** in Settings. Access also expires after 15 minutes without agent activity. Lock/expiry stops agent-owned CAN scans, frame sends, and captures and finalizes partial evidence; the paired CAN journal records why a scan stopped. Use `stop_operation` to stop earlier. The app shows the current operation and trace gap count in Settings.

The PIN is an in-app access gate. It does not isolate the MCP process from other software already running as the same Windows user. Treat raw protocol replies and CAN frames as research evidence; a response alone does not establish a signal meaning or vehicle validation.

## Example task

> Check `get_status`, connect the selected Candlelight adapter in active mode, start a CAN capture, then run the exact Mode 21 range and route I provide. Stop the scan, export the evidence, and report the request range, route, positive/negative/no-response counts, trace gaps, and the saved CSV/JSONL paths. Do not infer signal meanings from raw replies.
