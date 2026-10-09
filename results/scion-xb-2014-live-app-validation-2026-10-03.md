# 2014 Scion xB Toyota PID direct validation — 2026-10-03

## Outcome

A bounded direct adapter probe tested the 15 distinct Toyota Service 22 DIDs present in the local research database, using the same detected ISO 15765-4 CAN 11/500 protocol and the LogNook SX STPX transport path. No positive DID values were returned.

- At the live ECU route used by LogNook (7E0 request, 7E8 response), all 15 requests received 7E8 03 7F 22 11: UDS negative response NRC 0x11 (service not supported). The official AUTOSAR Diagnostics specification describes NRC 0x11 as serviceNotSupported.
- At the Toyota candidate database route (700 request, expected 708 response), all 15 original requests returned NO DATA while AT SH remained 7DF. A later single-DID retest of 1002 after AT SH 700 also returned NO DATA.
- No candidate returned a usable value. The 15 app-route negatives are valid Service 22 rejections; the original 700-route NO DATA results may have been affected by the receive filter. DID 1002 still produced NO DATA with AT SH 700. No samples were decoded or recorded.
- No source code was changed. No reset/wake, NVM, DTC-clear, actuator, or ECU-write command was sent. The app’s temporary volatile echo/header settings were restored and acknowledged.

## Revision and preserved changes

- Fetched origin/codex/grouped-toolbar-themes.
- Tested source base: e583126e8f98fca8fd9011cd924e45f1d67eaeed; the main checkout HEAD and fetched origin tip match.
- A detached QA worktree at that exact commit ran cargo test --workspace: 87 passed, 0 failed. Generated Tauri schema drift from the test build was restored in that isolated worktree.
- The main checkout had pre-existing local changes, left untouched: PID bitmap parsing for 0x60–0xE0, its regression test, and a Scion bitmap fixture. They do not add Toyota OEM signal support.
- The app window was closed by the owner to free COM4. Direct adapter tests ran outside LogNook as requested; the app was not relaunched for this probe.

## Machine and adapter

- OS: Windows 11 Home, 64-bit, 10.0.26200, build 26200.
- Rust/Cargo: rustc 1.99.0, cargo 1.99.0, active toolchain stable-x86_64-pc-windows-msvc.
- Visual Studio Build Tools: 17.14.37710.0; MSVC compiler 19.44.35229 (toolset 14.44.35207).
- Node: v24.19.0. npm is not available in this shell or the bundled Node runtime; documented X:\ toolchain location is not mounted.
- The requested npm run test:ui, npm run build, and npm run tauri -- dev checks were not run. I did not install or alter system tools or use the open executable in place of a fresh dev launch.
- Windows PnP: FTDI USB Serial Port (COM4), USB hardware IDs USB\VID_0403&PID_6015&REV_1000 and USB\VID_0403&PID_6015, instance serial 113011166292; FTDI driver 2.12.36.20.
- Direct identity replies: STN1130 v5.10.1, OBDLink SX r4.3.1, serial 113011166292.
- Owner confirmed the vehicle was stationary in Park, parking brake set, ignition on, engine off. OBDwiz/FORScan were closed; the owner disconnected/closed LogNook before the direct test.

## Protocol and command sequence

The probe used COM4 at 115200 8-N-1, matching the adapter transport in crates/lognook-core/src/obdlink_sx/serial.rs. Identity and protocol checks matched the adapter flow:

1. Adapter-only queries: STI, STDI, STSN.
2. Generic supported-PID discovery needed to mirror LogNook’s CAN/ECU routing:
   - 0100 → 7E8 06 41 00 BE 1F A8 13
   - 0120 → 7E8 06 41 20 90 05 B0 15
   - 0140 → 7E8 06 41 40 FA DC A0 01
   - 0160 → 7E8 06 41 60 08 00 00 00
3. STPRS reported ISO 15765-4 (CAN 11/500) [AT]. The single ECU responder was 7E8, so the app-derived physical request header was 7E0.
4. The app’s volatile CAN setup was mirrored with ATE0, AT H1, AT SH 7DF.
5. Each Toyota request used STPX H:<header>, D:22<DID>, R:<frames>, T:250. Duplicate database rows sharing one DID were queried once. First pass used app route 7E0; second pass used the research rows’ 700 request header.
6. Restored AT H0, AT SH 7DF, ATE1; all three returned OK. COM4 was closed. The adapter protocol was not changed.

The OBDLink Family Reference and Programming Manual documents the STPX per-request header/data/timeout controls and ATSH header behavior: https://www.scantool.net/scantool/downloads/234/stn1100-frpm-preliminary.pdf. NRC 0x11 interpretation follows the AUTOSAR Diagnostics specification: https://www.autosar.org/fileadmin/standards/R23-11/AP/AUTOSAR_AP_SWS_Diagnostics.pdf.

## Toyota/Scion catalog status

The LogNook runtime catalog in crates/lognook-core/src/simulator.rs exposes generic SAE Service 01 signals; the picker is labeled Engine · Mode 01. It has no Toyota/Scion Service 22 DID definitions or selector. The Scion preset in C:\LogNookR\PIDs\data\example_presets.json is explicitly standard OBD only and requires ECU verification before Toyota OEM DIDs.

The local research database C:\LogNookR\PIDs\data\lognook_pids.sqlite contains 23 Toyota rows across 15 distinct DIDs. All have research_state=community_reference, vehicle_validation=not_tested, auto_request=0, model_filter=brand_reference, no year filter, and ecu_filter=EXACT_ECU_ID_REQUIRED. Its request/response headers are 700/708; that route produced no response on this Scion. Candidate rows are not model/year/ECU-validated definitions.

| Service/DID | Research name(s) | Unit | App route 7E0 | Research route 700 |
|---|---|---:|---|---|
| 22/0103 | ECU odometer | km | NRC 7F 22 11 | NO DATA |
| 22/1002 | 12V starter battery voltage | V | NRC 7F 22 11 | NO DATA, including one filter-aligned retest |
| 22/1017 | Fuel injection volume | L | NRC 7F 22 11 | NO DATA |
| 22/1018 | Port injector pulse width | ms | NRC 7F 22 11 | NO DATA |
| 22/102A | Learned fuel trim high, idle, low, mid-1, and mid-2, bank 1 | % | NRC 7F 22 11 | NO DATA |
| 22/1041 | Coolant temperature at ignition; coolant temperature initial | °C | NRC 7F 22 11 | NO DATA |
| 22/1046 | Cooling fan duty | % | NRC 7F 22 11 | NO DATA |
| 22/105E | Engine ECU internal temperature | °C | NRC 7F 22 11 | NO DATA |
| 22/1062 | Exhaust VVT OCV duty | % | NRC 7F 22 11 | NO DATA |
| 22/106F | Intake VVT target, bank 1 | deg | NRC 7F 22 11 | NO DATA |
| 22/1071 | Exhaust VVT target, bank 1 | deg | NRC 7F 22 11 | NO DATA |
| 22/1087 | Intake VVT change, bank 1 | deg | NRC 7F 22 11 | NO DATA |
| 22/1088 | Exhaust VVT change, bank 1 | deg | NRC 7F 22 11 | NO DATA |
| 22/10A5 | Misfire count, all cylinders | count | NRC 7F 22 11 | NO DATA |
| 22/10A7 | Misfire count, cylinders 1–4 | count | NRC 7F 22 11 | NO DATA |

The POC plan additionally lists Toyota enhanced concepts such as idle control, knock correction, transmission temperature, and body/ABS signals, but provides no Scion xB identifiers or decoders for those concepts. They were not sent.

## Single DID follow-up with receive filter aligned

The OBDLink manual states that, in automatic filtering mode, the ISO 15765 flow-control filter updates when ATSH changes the transmit header. The original 700 pass kept AT SH 7DF while STPX overrode only its transmit ID; a 708 reply could have been filtered. To resolve that concrete uncertainty for one DID, the owner requested a quick retest of battery-voltage DID 1002.

The probe confirmed the same SX identity and CAN 11/500 protocol, sent ATE0, AT H1, and AT SH 700 (each acknowledged OK), then sent exactly one read request: STPX H:700, D:221002, R:1, T:250. The result remained NO DATA. It restored AT H0, AT SH 7DF, and ATE1 (all acknowledged OK), then closed COM4. No value was returned; no other DID was retried with the aligned filter. Automatic-filter mode itself was not independently queried, so the remaining 14 research-route candidates retain that limitation.
## Test counts, duration, and other steps

- Toyota DIDs: 15 unique requests on 7E0, each received one negative response; 15 unique requests on 700, each returned NO DATA; DID 1002 alone was retried with AT SH 700 and again returned NO DATA.
- Decoded values/samples: 0; no sample rates can be reported. Duplicate names for 102A, 1041, and 10A7 share the same DID request and response.
- Per-request STPX timeout: 250 ms. The three probe invocations took about 14.7 seconds combined including identity, support discovery, setup, restoration, and close; individual response durations were not recorded.
- Adapter/serial prompt timeouts: none. No vehicle values, recording, replay, or reconnect were attempted in this Toyota-only direct probe.
- No app UI error occurred because the app was disconnected and closed for direct adapter access.

## Evidence

- Raw app-route transcript: results/scion-xb-2014-toyota-did-probe-2026-10-03.log
- Raw research-header transcript: results/scion-xb-2014-toyota-did-header700-2026-10-03.log
- Research sources inspected: C:\LogNookR\PIDs\data\lognook_pids.sqlite, C:\LogNookR\PIDs\data\example_presets.json, C:\LogNookR\PIDs\docs\02_VEHICLE_POC_PLAN.md.
- Project references read: PROJECT_STATUS.md, docs/hardware/2026-10-02-sx-ford.md, LogNook_Blueprint/08_POC_VEHICLES.md.


## Follow-up assessment: exact-model signal data

The local source manifest says no 2014 xB enhanced request was physically verified in this package. A fresh inspection of the upstream Scion-xB default signal set found a populated community profile, including Toyota Service 21 entries. Its generation metadata identifies the 2012–2015 E150 xB with the 2AZ-FE engine. For example, the profile specifies engine request `21 54` at CAN header 7E0, response 7E8, decoding four cylinder speeds and average engine speed in rpm at a listed 0.25-second interval. These are community references and were not physically validated on this particular ECU. Links: [Scion-xB repository](https://github.com/OBDb/Scion-xB), [default signal set](https://raw.githubusercontent.com/OBDb/Scion-xB/main/signalsets/v3/default.json), [generation metadata](https://raw.githubusercontent.com/OBDb/Scion-xB/main/generations.yaml).

The Mode 22 results above still stand: 7E0 returned NRC 0x11 for those Service 22 requests, and the one filter-aligned 1002 request at 700 returned NO DATA. They do not test or reject Service 21. Further provenance research makes the profile's read-only `21 01` common-data block the preferred initial probe to 7E0 with expected response from 7E8; compare its common values with generic Mode 01. `21 54` remains a secondary RPM candidate. Neither entry specifies a diagnostic-level transition. No Mode 21 request was sent while the car was disconnected.


## Cross-check against another 2014 Toyota

The OBDb Toyota-RAV4 repository covers the 2013–2018 RAV4 generation. Its 700/708 Service 22 set contains the same candidate DIDs tested here, but these definitions are not xB physical validations. The RAV4 profile also has Service 21 entries for other ECU addresses/modules. Separately, the current Scion-xB profile contains its own 7E0/7E8 Service 21 engine entries, including `21 54`; those were not included in the local LogNook catalog and were not tested in this probe. Sources: [RAV4 signal set](https://raw.githubusercontent.com/OBDb/Toyota-RAV4/main/signalsets/v3/default.json), [RAV4 generations](https://raw.githubusercontent.com/OBDb/Toyota-RAV4/main/generations.yaml), [Scion-xB signal set](https://raw.githubusercontent.com/OBDb/Scion-xB/main/signalsets/v3/default.json).


## Diagnostic-session hypothesis

UDS defines DiagnosticSessionControl service 0x10; subfunction 0x03 is commonly used for ExtendedDiagnosticSession. Such a session can make additional services available on ECUs that implement it. AUTOSAR documents this sequence in its diagnostic-system test specification: [AUTOSAR system tests](https://www.autosar.org/fileadmin/standards/R25-11/AP/AUTOSAR_AP_TR_SystemTests.pdf).

The observed response was 7F 22 11. AUTOSAR distinguishes NRC 0x11 (serviceNotSupported) from NRC 0x7F (serviceNotSupportedInActiveSession): [AUTOSAR DCM specification](https://www.autosar.org/fileadmin/standards/R23-11/CP/AUTOSAR_CP_SWS_DiagnosticCommunicationManager.pdf). That points more strongly to Service 22 being unsupported at the responding endpoint than to a session-only restriction, though an OEM implementation could differ.

I found no 2014 xB/2AZ-FE document specifying a session transition or Mode 22 access sequence. The Toyota bulletin identifies the vehicle and Techstream workflow, but publishes no raw session command: [Scion S-SB-0015-14](https://static.nhtsa.gov/odi/tsbs/2015/MC-10134472-9999.pdf). No session-control request was sent. The next useful identification datum is the exact ECM calibration ID, obtained through a standard read-only vehicle-information path, then matched to a documented Scion-specific request definition.


## Wider Toyota model-year comparison

The OBDb Toyota-Camry profile covers the 2011–2017 XV50 generation and also lists several Mode 22 requests at 700/708, including DID 1002. Its filters/debug metadata remain model-specific and the entries are not Scion xB validations: [Camry signal set](https://raw.githubusercontent.com/OBDb/Toyota-Camry/main/signalsets/v3/default.json), [Camry generations](https://raw.githubusercontent.com/OBDb/Toyota-Camry/main/generations.yaml). The RAV4 profile likewise includes both Mode 21 module data and Mode 22 engine candidates. These examples show that 2008+ Toyotas do not share one universal diagnostic service, session, or DID map.

The inspected Scion-xB Service 21 entry `21 54` does not specify a required pre-session command. The xB response `7F 22 11` is evidence only about Service 22 at the responding endpoint; it says nothing about whether Service 21 works.


## Correction: Service 21 candidate for the Scion xB

The owner corrected the target service to Mode/Service 21. A fresh read of the upstream OBDb Scion-xB profile confirms explicit Service 21 request definitions. The profile lists `21 54` for average engine speed and per-cylinder speeds: request header 7E0, expected response 7E8, rpm decoder, listed profile interval 0.25 s. This interval is not a measured Scion rate. The better initial candidate after inspecting source provenance is `21 01`, described below. The OBDb schema defines `21` as a Service 21 PID and `dbg` as a response-debug flag; validation evidence must be inspected separately: [Scion-xB command](https://raw.githubusercontent.com/OBDb/Scion-xB/main/signalsets/v3/default.json), [OBDb command schema](https://github.com/OBDb/.schemas/blob/main/signals.json).

This is a profile candidate, not a completed physical result: `21 54` was not sent because the owner had disconnected the vehicle/adapter. The earlier Service 22 tests do not answer whether this Service 21 request works. The local LogNook runtime still exposes only generic Mode 01, and its local research database did not contain this Service 21 entry. No application source was changed.

## Mode 21 feasibility and next vehicle test

Source provenance matters: OBDb Scion-xB commit `d77a1cdde8706f0d14386096404b67bc6dc4669c` seeds 213 requests (107 Service 21 requests across modules) from Toyota family profiles. Its commit message explicitly calls them debug candidates, with none confirmed by the existing xB captures. It attributes `7E0 / 21 01` to a 27-data-byte Camry layout. Thus the xB repository name is not proof of support on this 2014 ECU. Source: [profile seed commit](https://github.com/OBDb/Scion-xB/commit/d77a1cdde8706f0d14386096404b67bc6dc4669c).

Engine provenance limitation: the commit attributes its 2010–2011 Camry references to 2AZ-FE, but those captures do not include an independently verified engine/ECM identity. Toyota's North American 2010 Camry documentation says its 2.5-litre engine replaced the previous 2.4-litre engine. Consequently these references establish decoder behavior on the captured Toyota responses, not verified same-engine compatibility with this xB: [Toyota Canada 2010 Camry changes](https://media.toyota.ca/en/whats-new/2009/whats-new-2010-toyota-camry.html).

Offline evidence: reassembled the first public 2010 and 2011 Camry `21 01` captures in memory and checked five common decoded values against each fixture's expected values. Both matched. Each capture contains five CAN frames and a declared 29-byte ISO-TP payload: `61 01` plus 27 data bytes. These are Camry references, not Scion measurements. Pinned sources, raw reference frames, comparison results and candidate identifiers are saved in `results/scion-xb-2014-mode21-research-2026-10-03.json`.

| Common field in the candidate 21 01 block | Zero-based data offset after 61 01 | Reference decoder |
|---|---:|---|
| Intake air temperature | 6 | byte - 40 °C |
| Coolant temperature | 8 | byte - 40 °C |
| Engine speed | 9–10 | big-endian u16 / 4 rpm |
| Vehicle speed | 11 | byte km/h |
| Throttle position | 14 | byte × 100 / 255 % |

Prepared bounded vehicle sequence, not executed:

1. Owner reconnects the SX to the stationary Scion, in Park with parking brake, ignition on and engine off. Discover its COM port again and ensure LogNook/other clients release it before direct access.
2. Reuse the established SX serial transport and already confirmed CAN 11/500 route. Record identity and paired generic Mode 01 common readings. Send one documented candidate request using `STPX H:7E0, D:2101, T:250` with the normal carriage-return terminator. Omit an assumed `R:1` during discovery; receive until the adapter prompt under the existing 2-second / 4096-byte host bounds. No reset, protocol switch, diagnostic-session change or write request is specified.
3. Accept only a complete ISO-TP message from 7E8 with positive service/local ID `61 01`. Check declared length, frame sequence and field bounds before decoding. Compare the five common values with the paired Mode 01 readings. A negative response, missing data, incomplete frames or implausible values stops the probe and is recorded verbatim.
4. After a successful block read, a brief repeated-read test can measure request rate, decoded values/s, per-field timestamps and gaps. All values from one block share one measurement event; a flat parked value is not automatically stale. Idle validation requires the owner to operate the engine outdoors, and would distinguish matching engine-off zeros from a correctly changing RPM decoder.
5. Only after the first block is validated, consider documented `21 E4` (injector pulse width), `21 3C` (injection data), `21 45` (misfire counters), and `21 82` (transmission temperatures, if applicable). Each identifier and decoder remains unverified on this Scion until its actual response is captured. No identifier scan is prepared.

The official OBDLink manual documents STPX's per-request data/header/timeout and segmented messaging, plus automatic flow-control filtering for the normal 7E0/7E8 CAN pair: [OBDLink FRPM, STPX and ISO 15765 reception](https://www.scantool.net/scantool/downloads/678/obdlink_frpm_e.pdf). No special preceding session command is specified by the candidate entries; absence of a stated precondition is not proof about every Toyota ECU.

Application feasibility: `HardwareAdapter` already returns `supported_signals` and `Sample` values, and `SignalDefinition` already contains an 8-bit service and PID. The current SX implementation nevertheless validates against `standard_obd_signals`, hardcodes Service 01 requests and widths, and rejects decoders whose service is not 01. Therefore the existing contract can accommodate Service 21, but inserting a Service 21 catalog row alone will not work. The smallest later implementation would reuse serial/ISO-TP handling, add a separate validated Service 21 block decoder, request each selected block once, and emit its selected fields as normal samples. Mode 01 support bitmaps must not establish enhanced-PID support. Only physically verified Scion fields should become selectable; actual Scion raw responses should become regression fixtures, covering complete/truncated frames, negative replies and value comparison. No such source change was made during this research.

