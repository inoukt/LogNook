# Development stages and proof-of-concept validation

## Stage 0 — requirements freeze, fixtures, safety
Lock shared sample/event types and minimal connection contract. Obtain at least one adapter of each family plus a real ECU or legitimate bench simulator. Create a reusable timestamped scripted ECU simulator covering variable delay, stale values, malformed responses, multiple ECUs and unplug/replug. Store independently captured reference frames as legal test fixtures. Write packaging/licensing inventory and establish strict read-only vehicle tests.

## Stage 1 — vertical slice
Connect one known-good generic USB ELM327 or OBDLink SX to simulated/real generic CAN OBD; show supported PID selection, live RPM/coolant graph, marker, Start/Stop, exact timestamped `.lognook`, live CSV export, reopen/replay and crash recovery. In parallel build automated tests for parser and scheduler. A usable vertical slice before elaborate UI theming prevents architecture drift.

## Stage 2 — four adapters
Add STN fast path and robust ELM clone profiling; standalone `gs_usb` raw CAN, SLCAN fallback, DBC import; 32-bit isolated J2534 helper and Tactrix official driver discovery. Maintain common UI and recording path. Test 11/29-bit CAN, varying bitrates, multiple responders, sustained load, USB hotplug, adapter contention and power loss. Document precise unsupported firmware features.

## Stage 3 — technician suite
Auto-record and configurable pre-trigger ring buffer; editable markers; hotkeys; live graph/list modes; CSV importer, resampling/export, scatter/heatmap/bar; formulas/conversions/tare; per-PID plot settings; known-good library; alert tones and workspace restore. Implement DTC *read* workflow only after deterministic logging is stable.

## Stage 4 — enhanced diagnostics and integrations
Introduce profile-driven UDS read-only DIDs/ECU browser, authorized OEM definitions, K-line validation, optional J1939. Add per-user Google Drive authorization and resumable upload. Offer an export/open-website Datazap path immediately, but do not advertise direct upload until an approved API or agreement has been verified. Add opt-in AI diagnostic telemetry bridge and structured POC reports.

## Stage 5 — broader operating systems and remote
Linux Native/SocketCAN adapter; package and permission rules; remote LAN view-only dashboard. Later, authenticated, separately enabled remote operator controls; Bluetooth/Wi-Fi/DoIP adapters follow the common interface contract.

The unprioritized ideas in [`Addons for later.txt`](../Addons%20for%20later.txt) also include a floating/separate notepad, saved graph annotations, phone sensor channels, phone-camera video synchronized to a session, and virtual-dyno feasibility. Treat these as candidate follow-on work: define data format and session synchronization before implementing them; remote phone control requires an explicit authenticated design.

## Test matrix (record firmware, vehicle ECU and observed conditions)
- Known field vehicles: **2011 Ford Flex 3.5L EcoBoost**, **2014 Scion xB 2.4L 2AZ-FE**; Subaru make/model/year/ECU variants as test access permits. See `08_POC_VEHICLES.md` for the actual sequence and conditional signal selection.
- Personally available adapters: Tactrix OpenPort 2.0, OBDLink SX, generic ELM327 (specific firmware/driver revisions must still be recorded). RH-02 physical hardware is not yet confirmed.
- Devices: OpenPort 2.0 with official J2534; OBDLink SX FTDI; known-original/suspect ELM clones; RH-02 candleLight; CANable SLCAN.
- Platform: Windows 10 and 11 x64 on at least two USB chipsets; Linux engine/adapter continuous integration (later physical tests).
- Protocol: generic CAN 11/29-bit, real multi-ECU requests, ISO-TP segmentation, variable ECU timing, K-line/J1850 where the device is electrically capable.
- Failure: missing vendor DLL, wrong-bitness DLL, permission denied, busy COM port, USB disconnect mid-CSV, slow/unresponsive ECU, truncated serial response, oversized CAN burst, application force-close, disk full, sleep/resume, corrupt CSV.
- Concurrency: 5/25/75 selected signals with priorities; sustained 30-minute diagnostic poll; 60-minute raw-CAN capture; 8-hour synthetic replay and export; UI interaction under load.

## Exit criteria for each adapter
Adapter discovery without guessing; connect and correctly identify bus/ECU; validate all returned PID formulas against independent known reference; record raw and decoded data with time and quality; show achieved rate/timeouts/drop counters; survive detach/reattach without fabricated samples; verify 30-minute stable recording and integrity of reopen/export. Publish measured rates rather than 'fastest' marketing numbers.

## Decisions that require real evidence
OBDLink SX ST command support by its actual firmware; OpenPort DLL bitness and documented J2534 capabilities in installed version; RH-02 firmware and WinUSB binding; licensing for bundling drivers and database definitions; exact Datazap approved upload method; OEM data-pack rights; high-load CAN loss behavior. Track each as Open / Tested / Validated / Unsupported in a short hardware capability registry.
