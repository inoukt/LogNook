# Local Agent Control for LogNook

**Status:** design for review
**Date:** 2026-10-07

## Goal

Let a local AI agent inspect and operate LogNook through a small, documented tool API. It should be able to manage PID selections, inspect OEM and Mode 06 information, run explicit Workbench tests, and observe the adapter traffic and results. The user unlocks Agent Access with a PIN; after unlock, the agent can carry out the user’s scoped task without a confirmation dialog for every tool call.

The first version is same-PC only. The running LogNook app remains the owner of every adapter connection and operation.

## Decisions

- Use a bundled MCP server launched by the agent host over **stdio**. On Windows, it talks to the running app through a per-user named pipe; use the OS-native local IPC equivalent on other supported desktop systems. Do not open an HTTP or LAN listener.
- Keep adapter I/O in the existing LogNook acquisition and Workbench owners. The agent server forwards typed requests; it never opens a serial, Bluetooth, J2534, or CAN device itself.
- Agent Access is locked by default. The user enters the PIN in LogNook, never in an agent prompt. Unlock lasts 15 minutes after the last agent action; manual lock, timeout, or app exit revokes access and stops the active agent operation.
- Allow one active agent operation per adapter. Reuse existing cancellation and progress reporting.
- Connecting an adapter does not start a scan or transmit a diagnostic request. The agent must start each operation explicitly from the user’s task.
- Do not add a network service, a generic Tauri-command executor, a second adapter worker, or a new catalog/measurement pipeline.

## User workflow and instructions

1. Install LogNook with its bundled local agent server.
2. In **Settings → Agent Access**, set a local PIN, choose **Copy agent setup**, and add that setup to the selected agent host. Keep the PIN in LogNook; do not paste it into the host or give it to the agent.
3. Open LogNook, select/connect the adapter, then unlock Agent Access. The app shows whether the agent is connected, the selected adapter, the active operation, and the lock timer.
4. Give the agent a specific task: vehicle or bench context, adapter, operation, route/range, pacing, and what evidence to save. The user can watch progress, press **Stop**, or lock Agent Access at any time.
5. Review or export the trace and test evidence from LogNook when the task finishes.

Example task: “Use the connected RH-02 in CAN Workbench. Capture traffic for 30 seconds while I toggle traction control six times. I’ll say ‘mark’ after each toggle. Compare the marked windows, identify IDs and changing bytes, then save the evidence report. Do not transmit a CAN frame.”

The PIN is a local-use gate, not a defense against malicious software already running as the same Windows user. Show the lock state clearly and never log or return the PIN.

## Agent instructions

Expose these instructions in the MCP server description and in a short in-app **Using Agent Access** guide:

1. Call `get_status` first. Confirm Agent Access is unlocked and identify the connected adapter and active operation.
2. Follow the user’s requested scope. Use only adapter IDs, routes, PIDs, tests, ranges, and pacing confirmed by LogNook or specified by the user. Never guess an ECU route.
3. Use the typed LogNook tools below. Do not call shell commands, invoke arbitrary Tauri commands, or open a device independently.
4. Start one operation at a time on an adapter. Read progress and trace events; report progress, errors, and typed outcomes. Stop promptly when asked, when the adapter disconnects, or when the unlock is revoked.
5. Preserve raw TX/RX bytes and ECU/CAN identity in evidence. Report positive replies, NRCs, timeouts, malformed replies, and wrong-route replies separately. A raw response is research evidence; it does not prove a decoder or vehicle support.
6. Normal PID acquisition follows the existing auto-request and vehicle-validation rules. Workbench scans and a single operator-configured CAN frame are available only as explicit user-directed Workbench actions. Do not create automatic programming sequences, change ECU sessions/security, clear codes, flash firmware, or operate actuators.
7. Before finishing, summarize what was tested, what responded, what did not, timing/errors, and the evidence file or capture ID. Say when trace coverage was incomplete.

The agent does not need a second approval for each request after the user unlocks Agent Access. A tool is rejected if the app is locked, the requested capability is unavailable, or its typed arguments are invalid.

## Tool surface

Keep the tool list small and domain-specific. Names below describe capabilities, not a requirement to mirror existing Tauri command names.

| Tool | Purpose |
| --- | --- |
| `get_status` | Lock state, adapter state/capabilities, current operation, and recent errors. |
| `list_adapters` / `connect_adapter` / `disconnect_adapter` | Discover available devices and use the existing adapter owner. Connecting never starts tests. |
| `list_signals` / `set_signal_selection` | Search standard PIDs and set their existing log/graph selections. Normal polling eligibility remains unchanged. |
| `search_oem_candidates` / `try_oem_candidate` | Search source-backed catalog entries and perform an explicit one-shot test; return raw and typed results. |
| `list_mode06_tests` / `try_mode06_test` | Show tests supported by the active adapter and run a selected one-shot test. |
| `workbench` | Start/stop capture, add an event marker, run a configured CAN or Mode 21/22 scan, or send one manually configured CAN frame. Require explicit typed settings and preserve the exact configuration. |
| `get_trace` | Read or follow adapter communication events from a cursor; report gaps if a bounded live queue overflowed. |
| `stop_operation` / `export_evidence` | Cancel the current action and save its trace/capture/report through existing export paths. |

`workbench` accepts a closed set of action types, not arbitrary commands. Numeric scan ranges may cover the full supported range when the user explicitly asks. Scans are paced, cancellable, serialized through the existing adapter owner, and recorded with their exact configuration. Candidate ranking may put vehicle/profile-matched and source-backed items first, but must show where a result came from and must not silently skip a user-requested range.

## Communication trace

The trace represents exchanges at the adapter-driver boundary, so it can explain what LogNook actually sent and received. Include:

- wall-clock and monotonic timestamps, adapter/provider, operation, direction, and elapsed time;
- exact request and response bytes when the driver exposes them, CAN/ECU route and identifiers, and a typed outcome;
- timeouts, disconnects, parse errors, provider/worker errors, and explicit “not available” details when a driver cannot expose raw bytes.

Cover the existing SX serial, Bluetooth ELM327/A30, J2534/OpenPort worker, and Candlelight/RH-02 paths as each path is instrumented. For CAN sniffing, the agent reads the existing Workbench capture stream rather than starting a competing reader. Show trace coverage and dropped-event counts; never imply a complete trace if events were lost or a driver hid bytes.

Keep a bounded recent in-memory trace for live inspection. A user-started capture/evidence export retains complete available traffic through the app’s asynchronous evidence writer; add the smallest shared writer needed for adapter paths that do not have one yet. Trace display and agent subscribers must not block acquisition or drop recorded measurements. If a live subscriber falls behind, expose the gap and let it resume from the capture/evidence file.

## PIN and local IPC

- PIN setup and unlock happen only in the LogNook UI. Store a verifier, rate-limit failed attempts, and exclude the PIN from logs, IPC payloads, and MCP results.
- The local IPC endpoint is available only while Agent Access is unlocked and is restricted to the current OS user. Lock, timeout, app close, or lost session revokes it and cancels its active Workbench operation.
- Bind the IPC session to one running LogNook instance. If the app is absent or locked, return a clear error with the next user action; never launch a hidden scan or create a second adapter owner.
- The first implementation supports a single local agent host. Remote control, multi-user authorization, and LAN access are out of scope.

## Acceptance checks

- The user can copy setup instructions, configure one agent host, set/unlock/lock the PIN, and tell whether an agent is connected without reading developer documentation.
- The agent receives a concise usage guide and can determine lock, adapter, capabilities, progress, and trace gaps through `get_status` and typed tools.
- Calls fail clearly while locked or when LogNook is closed. Locking during a scan cancels it and leaves partial evidence with a cancellation reason.
- Agent connect/disconnect and Workbench calls reuse the existing serialized adapter owner; no duplicate hardware connection or background scan is created.
- Tests cover PIN lock lifecycle, local IPC availability, typed argument rejection, operation serialization/cancellation, trace fidelity and gap reporting, and tool-result classification for success, NRC, timeout, malformed reply, and disconnect.
- User and agent guides include a copyable task example and explain how to stop, lock, and export evidence in plain language.

## Out of scope

LAN or laptop-to-PC control, arbitrary command execution, automatic adapter scans at startup, silent catalog promotion, automatic multi-step ECU writes, ECU session/security changes, DTC clearing, actuator control, and firmware flashing.
