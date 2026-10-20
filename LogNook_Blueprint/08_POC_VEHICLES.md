# Physical POC vehicle test plan — known shop vehicles

Status: vehicle test matrix and protocol; it is not a current results page. The opening plan predates later adapter-to-car measurements. See [current Project Status](../PROJECT_STATUS.md) and the dated reports under `docs/hardware/` and `results/` for actual observations. Add new results there with the code revision and conditions.

## Equipment currently reported
- Tactrix OpenPort 2.0 — vendor J2534 driver plus 32-bit worker as needed.
- OBDLink SX — USB serial, with its own verified ST-command capability profile.
- Generic USB ELM327 — identity and real command probing; assume no optional command works until tested.
- RH-02 / Candlelight / CANable: implementations and user-reported adapter availability vary; this plan does not identify a specific tested unit. Record hardware identity and actual bus/vehicle tests in a dated report before claiming support.

Use the same immutable acquisition, parser, recorder and chart events through all three hardware backends. Record OS build, USB hardware ID, driver version, adapter firmware, protocol negotiated, ECU identifiers, PID set, polling configuration and raw response traces for each test.

## Vehicle A: 2011 Ford Flex — 3.5L EcoBoost V6

Manufacturer literature confirms twin-turbocharged direct-injection EcoBoost for this model. This is the turbocharged engine-performance and vehicle-module test platform.

**Read-only initial scenario:** engine-off, ignition-on discovery; then warm-idle generic Mode 01 scanning and a synchronized acquisition of each *actually reported supported* standard signal: RPM, coolant temperature, intake air temperature, throttle, MAP, fuel trims, calculated engine load, vehicle speed and any supported fuel-rail or oxygen/lambda measurement. Do not treat these as guaranteed; query supported-PID bitmaps and validate data against a reference instrument/known values.

**Tuning-oriented scenario:** if manufacturer-enhanced definitions are lawfully acquired and matched to the actual ECU calibration, test boost-related measurements, rail pressure, knock/ignition corrections, requested-vs-actual values and transmission observations. Never present inferred boost (`MAP - barometric pressure`, where appropriate) as equivalent to an OEM measured boost parameter; preserve source and units.

**Network limitation:** OBDLink SX handles legislated OBD but its manufacturer explicitly recommends MX+ for proprietary Ford MS-CAN. Neither an SX nor an ordinary ELM clone should imply all-module coverage. Do not claim Tactrix/OpenPort gives universal Ford MS-CAN access without confirming physical pins/routing, hardware capability, installed J2534 API and model-specific module addresses. Future switchable physical adapters must include electrical and bus-safety review.

## Vehicle B: 2014 Scion xB — 2.4L 2AZ-FE

Toyota product documentation and a Scion service bulletin identify the 2.4L engine and 2AZ-FE family. This is the clean cross-manufacturer baseline and Toyota/Scion enhanced-data research platform.

**Read-only initial scenario:** the same standardized engine-off/ignition-on, warm-idle, controlled throttle and safe stationary tests with the SX, ELM and Tactrix. Discover supported standard PIDs; where offered compare RPM, coolant, intake temperature, throttle, MAF/load, short/long fuel trim and lambda/O2 readings. Keep a source-aware record of every actual supported PID and missing signal.

**Enhanced scenario:** test Toyota/Scion engine and transmission read definitions only from a reviewed source with exact request addressing; treat each as unverified until the xB ECU responds and the decoder is checked. If a test requires session/security access or another state-changing step, use the exact per-test gate in root `AGENTS.md`. Toyota Techstream references are comparison material, not redistribution permission.

## Vehicle C: Subaru — rotating shop samples, exact models not yet identified

Use Subaru access to benchmark SSM protocol detection, source-validated logging definitions, safe high-priority tuning presets, CSV interchange with RomRaider and the effect of expanding selected-PID sets on achievable poll rate. Test both relevant older K-line and newer CAN vehicles *when actual access and compatible hardware permit*. Do not infer an SSM generation, ECU memory address or engine variant from the badge alone. Record the ECU identifier, calibration/software revision and provenance of each parameter definition.

## Repeatable, comparable bench + field sequence

1. **Dry run without a car:** virtual serial/ECU, ISO-TP fixtures, bad responses, reconnect and forced mid-recording close; assert parser correctness and intact `.lognook` and CSV exports.
2. **Scion baseline first:** OBDLink SX -> ELM -> OpenPort with identical discovered/overlapping generic PID sets; time USB discovery, initialization, ECU response, first accurate live sample and first recorded sample separately.
3. **Ford generic baseline second:** run the same adapter order, then a tuning-relevant subset of *supported* PIDs. Avoid demanding unavailable enhanced PIDs, especially Ford MS-CAN.
4. **Subaru enhanced track:** only after ECU identification and tested definitions; use OpenPort/J2534 where supported, and compare a reference logger on matching PID sets if possible.
5. **Reconnect/failure test:** unplug USB, unplug/replug the OBD connector in safe conditions, power-cycle the simulator and stop/start ECU replies. A lost signal must be visibly stale, a gap recorded, no fake zeros or interpolated continuity silently inserted.
6. **Measurement record:** for every run store median/p95 first-sample time, each signal's measured median/p95 inter-sample interval, effective requests/s, response error/retry counts, UI preview frame rate, recorder backlog, memory use and raw-vs-decoded verification outcomes.

Initial acquisition and baseline measurement tests are read-only and stationary. This protocol-validation workflow is separate from the developer Test Workbench bench mode described in root `AGENTS.md`: the Workbench may sniff raw CAN, send operator-configured frames, and run selected scans without a per-request approval prompt. Those captures remain bench/research evidence and do not automatically validate or promote catalog definitions. The current Workbench scope does not add firmware flashing or automated ECU-programming workflows. If a measurement requires driving under load, use a second operator or an appropriate dyno; never operate a PC while driving.

## Acceptance
- One complete, actual-car vertical slice on Scion (connect -> select -> read -> graph -> mark -> record -> CSV -> reopen -> replay) before complex OEM expansion.
- Identical core recording/event behavior on SX, ELM and OpenPort; any driver-specific missing capabilities reported clearly.
- Ford generic logging and accurate disclosure of enhanced-module limitations.
- At least one verified Subaru ECU-specific definition and captured reference session **before** advertising supported Subaru enhanced coverage.
- Report real attained throughput and source evidence, not a single universal promised PID/s number.

Primary references: see `07_REFERENCES.md`; individual POC observations and fixtures will be stored with test identifiers, not added as unsupported claims here.
