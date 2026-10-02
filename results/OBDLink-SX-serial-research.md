# OBDLink SX serial research — integration brief

Status: research complete, **nothing physically tested**. No adapter, driver, or vehicle was
touched while producing this document. Every claim below is either (a) quoted from a vendor
primary source with section and page, or (b) explicitly marked **unknown** / a field-test
question. Nothing is inferred from clone hardware.

Reviewed 2026-10-02. Governing document: **OBDLink Family Reference and Programming Manual
(FRPM), Revision F, August 29, 2025** (78 pages; the manual's own page footer is `N of 78`).
Page numbers in this brief are the manual's printed page numbers, not PDF page indices.

---

## 1. Headline findings

1. **The SX is the STN1130, and it is classified `Obsolete` by the manufacturer.** FRPM §3.1,
   Table (p. 6) lists `STN1130 | Obsolete | OBDLink SX | Low cost OBD to USB`. §3.0 defines
   `Obsolete` as "The product is no longer sold, and firmware is **no longer in active
   development**." The product page independently says "THIS PRODUCT IS NOW DISCONTINUED".
   *Consequence:* the firmware on the bench unit is effectively frozen, so nothing may be
   assumed to exist. Capability probing is not a nicety here, it is the only correct design.
2. **The FRPM is family-wide and explicitly does not promise uniform support.** §4.0 Feature
   Highlights (p. 6) ends with "Support for all legislated OBD II protocols (**not available in
   all devices**)", and §7.1 classifies each AT command as `supported`, `deprecated`, or
   `not yet supported`.
3. **Many classic ELM327 commands are `deprecated` on this platform and have ST replacements.**
   Notably `ATST` (timeout), `ATDP`/`ATDPN` (describe protocol), `ATMA` (monitor all), `ATAR`,
   `ATPC`, `ATSS`, `ATTP`, `ATBI`, `ATRV`/`ATCV` (FRPM §7.1 Tables 1, 3, 5, pp. 9–10). An
   ELM327-centric client that assumes `ATST` changes the timeout can silently keep the wrong
   timeout — use `STPTO` instead (FRPM §8.6, p. 35).
4. **The SX remains correct for the read-only generic-OBD MVP, and only for that.** It
   supports all *legislated* OBD-II protocols (product page). It does **not** reach Ford
   MS-CAN / GM SW-CAN; the product page directs those users to the MX+, and the FRPM's device
   table attributes MS-CAN/SW-CAN only to STN1150/1151/1152/1170/2120/2255-class devices
   (FRPM §3.1–3.2, p. 6). Do not claim Ford enhanced coverage for the SX.

---

## 2. Serial operation as documented (facts, not assumptions)

### 2.1 Physical link, driver, COM port

| Fact | Source |
| --- | --- |
| The SX is a USB 2.0 device; supported host platforms Windows Vista+ and Android. | Product page, Specifications |
| OBDLink ships the device as an **FTDI Virtual COM Port** device; the SX support page lists "Drivers → FTDI Virtual COM Port Driver" and a separate "Windows Driver" link. | SX support page |
| Driver state is visible from the LED: **yellow LED on USB connect = ready**; **red LED = drivers were not installed properly**; green LED after key-on/engine-off = tool ready to use. | SX Windows Quick Start, steps 2 & 5 |
| Windows auto-installs in the normal case; if not, OBDLink's Windows driver link is the documented remedy. | SX Windows Quick Start, step 2 |
| OBDwiz "Auto-Detect" exists precisely because the port and the baud rate are not fixed: it "check[s] all known COM ports and baud rate options and auto-configures itself". | SX Windows Quick Start, step 7 |
| Firmware update on Windows uses the **STN Firmware Updater**; on Android the OBDLink app with a USB OTG cable. Windows users get no update notification. | Update OBDLink Adapter Firmware article (table) |
| Latest SX firmware archives: `scantool.net/downloads/updates/obdlink_sx/` (extract the archive, run `StnFirmwareUpdater.exe`). | Same article + SX firmware page |
| The adapter serves **one application at a time**. | Update-firmware article; Windows get-started article |

### 2.2 Default UART settings and line protocol (FRPM §6.0, p. 8)

- Three-wire UART, CMOS/TTL compatible. Defaults: **baud 115200 bps for USB OBDLink adapters**
  (9600 bps for bare ICs/modules; Bluetooth adapters vary and cannot be changed), **8 data bits,
  no parity, one stop bit**.
- Once powered and connected it prints the startup message `ELM327 v1.4b` followed by the
  prompt `>`.
- "**User software should always wait for the prompt before sending the next command or
  request.**" The prompt is the framing delimiter.
- Three command families: **AT commands**, **ST commands**, and **OBD requests**.
- "Only ASCII hexadecimal digits (0-9 and A-F) are allowed in OBD requests."
- "Only ASCII alphanumeric characters, numbers, backspaces, and the carriage return are
  accepted on the UART; spaces are ignored. **All commands must terminate with a carriage
  return (0x0D)**."
- Line endings: "By default, responses from the OBDLink are terminated with a carriage return
  (0x0D). `ATL1` can be used to have the OBDLink append line feeds (0x0A) to the carriage
  returns."
- "Sending a single carriage return character repeats the last command."
- §9.0 (p. 54) adds that current firmware **ignores 0x0A** to avoid false `STOPPED` errors —
  which means a stray non-LF character *does* abort the running command with `STOPPED`.

Consequently the default wire state is: **echo ON, line feeds OFF, prompt-terminated
responses, CR-terminated commands.** The FRPM's asterisk convention ("Asterisk (\*) next to a
setting means it's the default value", §7.1, p. 9) agrees: `ATE 1|0` shows the default as
echo-on, and `ATL 1|0` shows line feeds defaulting off. See the open question in §5 — confirm
by reading the actual bytes, not by reading the table's typography.

### 2.3 Baud rate changes

- Current method (firmware ≥ v2.0.1): `STSBR <baud>` → switch host to the new rate →
  `STI` to confirm communication → `STWBR` to save. | ScanTool blog "Switching Communication
  Baud Rate"
- Legacy method (v1.1.0+ / ELM327-based): `ATPP 0C SV xx`, `ATPP 0C ON`, `ATZ` (0C value
  map: 19.2k=D0, 38.4k=68, 57.6k=45, 115.2k=23, 230.4k=11, 500k=08). | Same article
- The same article records that OBDLink defaults moved from **38400 bps on v1.2.4 to 115200
  bps on v2.1.0**, matching FRPM §6.0's 115200 for USB adapters.
- **Do not confuse UART baud with OBD bus baud.** `STPBR`/`STPBRR` (FRPM §8.6, p. 35) set and
  read the *vehicle-side* protocol baud, and `STPBRR` "Returns **?** if the protocol is AUTO".

### 2.4 Safe device and firmware identification

| Command | Documented behaviour | Source |
| --- | --- | --- |
| `ATI` | Print ELM327 version ID string (status: supported) | FRPM §7.1 Table 1, p. 9 |
| `AT@1` | Print device description (supported) | FRPM §7.1 Table 1, p. 9 |
| `AT@2` | Print device identifier (supported) | FRPM §7.1 Table 1, p. 9 |
| `STI` | "Prints firmware ID string, in this format: **STN\<device_id\> vX.Y.Z**", where X.Y.Z is the firmware version | FRPM §8.4, p. 31 |
| `STDI` | Device ID string (also reported as a component of `STDIX`) | FRPM §8.4, pp. 30–31 |
| `STDIX` | Extended device information: device ID string, extended firmware ID string (STIX), manufacturer ID (STMFR), serial number (STSN), bootloader version, internal chip ID/revision, device init date, POR count, run times, engine cranks/starts. Example output for an OBDLink EX: `Device: OBDLink EX r1.0.0` / `Firmware: STN2230 v5.10.3 [2024.02.01]` / `Mfr: OBD Solutions LLC` / `Serial #: 223010296101` / `BL Ver: 4.0` / … | FRPM §8.4, p. 31 |
| `STSN` | Serial number (listed among `STDIX` contents) | FRPM §8.4, p. 31 |
| `STDICPO`, `STDITPO` | Power-on-reset count; POR timer (run time since last power cycle) | FRPM §8.4, p. 31 |
| `ATZ` / `ATWS` / `ATD` | Reset device / warm start / set all settings to defaults (all supported) | FRPM §7.1 Table 1, p. 9 |

The FRPM attaches an explicit stability warning to `STDIX`: "The information provided by this
command is subject to change in future firmware updates… users should review the latest
documentation to ensure accurate interpretation of the command's output." Treat `STI`/`STSN` as
the parseable identity and `STDIX` as capture-and-log-only text.

**Never send:** `STSDI` (one-time reprogramming of the device ID string, FRPM §8.4 p. 31),
`AT@3` (store device identifier, §7.1 p. 9), `STPX` (transmit arbitrary message on the OBD bus,
§8.6 p. 35), `ATPP xx SV/ON/OFF`, `STSBR`/`STWBR` (persist settings). All of these mutate
non-volatile state. They are out of scope for a read-only acquisition MVP even on the bench.

### 2.5 OBD request format, timeouts and error strings

- FRPM §10.0 "OBD Requests" (p. 54): "The OBDLink uses the same format for OBD requests as the
  ELM327. Please refer to the 'OBD Commands' section of the ELM327 datasheet for information."
  It then names the governing standards: **SAE J1979 / E (ISO 15031-5)**, SAE J2190, SAE J2178.
  *This is the vendor telling us the PID-level semantics are J1979/ELM327, not STN-specific.*
- Mode $01 is "Request Live Data" (OBD Solutions, "OBD Modes").
- Worked request/response examples from the vendor's own tutorial (OBD Solutions, "Reading
  Real-Time Data"): with the port at 115.2 kbps, `AT SP 0` → `OK`; `AT DP` → `AUTO`;
  `010C` → `SEARCHING: OK` then `41 0C 0F A0` (RPM = raw/4 → 1000 rpm); `010D` → `41 0D FF`;
  `0104` → `41 04 7F` (A/255×100 %); `0105` → `41 05 64` (A−40 °C). The tutorial stresses that
  the tester must *request* data; nothing is broadcast.
- Timeouts (FRPM §8.6, p. 35): `STPTO ms` sets the OBD request timeout (1–65535 ms, 0 =
  infinite); "**Default is 102 ms**" (controlled by programmable parameter PP 03).
  `STPTOT ms` sets the message-transmission timeout before the device prints `BUS BUSY`, with
  documented per-protocol defaults: J1850 PWM/VPW 300 ms, ISO 9141-2/ISO 14230-4 300 ms,
  **ISO 15765-4 (CAN) 50 ms**. `STPTRQ ms` sets the minimum gap between the last response and
  the next request (this is the P3 timing for ISO 9141-2/14230-4; default 56 ms there, 0
  elsewhere). `ATAT` (adaptive timing) is supported with default `auto1` (Table 5, p. 10).
- Protocol control (FRPM §8.6, p. 35): `STPO` open current protocol, `STPC` close, `STPR`
  print protocol number, `STPRS` print protocol string, `STPCB 0|1` auto check-byte
  calculation (default 1). AT-side equivalents: `ATSP protocol` "set protocol and save it"
  (supported, Table 5 p. 10) — note it *saves*, i.e. it writes non-volatile state.
- Documented error/status strings a serial parser must handle (FRPM §9.0, p. 54):
  - `NO DATA` — no response before timeout. Causes listed: unsupported request, filters,
    "**timeout set with ATST/STPTO is too short**", or a protocol the adapter cannot run.
  - `UNABLE TO CONNECT` — bus protocol not detected (vehicle not OBD-II compliant, ignition
    off, no power on the diagnostic connector, wiring).
  - `STOPPED` — a character arrived on the UART and interrupted the OBD command.
  - `<RX ERROR` — CAN peripheral saw a bad frame (documented as usually an incorrect baud rate).
  - `UART RX OVERFLOW` — host sent data too fast, most likely on ISO 9141/14230.
  - `OUT OF MEMORY`, `LV RESET` (brown-out, prints on recovery), `LP ALERT` (about to sleep).
  - Plus `SEARCHING: OK` (protocol search) and `BUS BUSY` seen in the tutorial/§8.6.
  - `?` is the unknown-command response (documented for out-of-range use in §8.6, and the
    standard ELM327 behaviour).
- Filtering (FRPM §11.0, pp. 54–58) is pass/block/flow-control and "backwards compatible with
  the ELM327" but is a *receive* concern; the read-only MVP does not need it.

### 2.6 Multi-ECU reality on the target vehicle

OBDLink's own Quick Start states: "On many vehicles, there are multiple on-board computers
(ECUs)… since OBD-II protocol dictates that you can only connect to one ECU at a time, it's
recommended you connect to the ECU reporting the most Supported PIDs first." Any supported-PID
scan must therefore expect **more than one responder** and must not silently merge their
bitmaps.

---

## 3. Proposed mapping onto `HardwareAdapter`

Current contract (`crates/lognook-core/src/adapter.rs`): `connect() -> ConnectionCapabilities`,
`read_samples(&[SignalDefinition]) -> Vec<Sample>`, `disconnect()`.

### `connect() -> ConnectionCapabilities`

1. Open the operator-selected COM port at **115200 8-N-1** (FRPM §6.0) and assert nothing that
   changes device state. *Unknown:* whether the banner only appears at power-up; if no prompt
   arrives within a bounded wait, send a bare `CR` (documented: repeats the last command) or
   `ATZ` as the recovery step, and record which one was needed.
2. Drain the startup banner (`ELM327 v1.4b`) and wait for `>` (FRPM §6.0).
3. Configure only session-scoped, non-persistent settings: `ATE0` (we own the framing; echo off
   avoids ambiguous parsing), `ATL0` (already the default), and decide `ATH1` vs `ATH0` — keep
   headers **on** when we need per-ECU attribution. Avoid `ATSP` if its "set protocol and save"
   behaviour writes flash; prefer the ST path (`STPO` after an auto-search) and verify with
   `STPRS`. **Probe, don't assume:** send each setup command, read the reply, and treat `?` or
   `STOPPED` as "unsupported on this firmware" rather than a fatal error.
4. Identity: `ATI`, `AT@1`, `AT@2`, then `STI` (expect `STN1130 vX.Y.Z`) and `STSN`; capture
   `STDIX` verbatim into the session log without parsing it. Build
   `DeviceDescriptor { stable_id: serial-based, hardware_family: "obdlink-sx", firmware: STI
   string, path: COMx }`. Prefer the STSN serial over VID/PID for `stable_id`.
5. Protocol: `ATSP0` (auto) equivalent, then read the negotiated protocol with `STPRS`
   (`ATDP` is deprecated). Record it in `ConnectionCapabilities.protocol`.
6. Timeouts: set `STPTO` explicitly and record whether it was accepted; fall back to the
   documented 102 ms default and to `ATAT1` if `STPTO` is rejected.
7. Supported-PID discovery: request `0100`, parse the 32-bit bitmask, continue with `0120`,
   `0140`, `0160` while the continuation bit says more ranges exist, and keep one bitmap **per
   responder**. Mark every candidate signal not present as explicitly unavailable rather than
   filling a fabricated value. Set `read_only: true`.

### `read_samples(&[SignalDefinition]) -> Vec<Sample>`

- Build an ASCII-hex Mode 01 request per signal (`010C`, `0105`, `010F`, `0111`, `010D`) plus
  `CR`. Grouping several PIDs into one request is a documented ELM327 capability but is
  single-ECU by construction; decide grouping only after the field test measures it.
- Parse response lines until `>`: strip echo, expect `4x <pid> <data…>`, retain the raw bytes
  in `Sample.raw_bytes`, convert with the J1979 formulas (RPM raw/4; coolant and intake A−40;
  throttle A×100/255; speed A), and use the CAN header (when `ATH1`) or the responder identity
  for `ecu_id`.
- Map `NO DATA` → `timeout`, `?` or an unrecognised line → `parse_error`, a partial/malformed
  frame → `parse_error`, `STOPPED`/`UART RX OVERFLOW` → adapter-level error, and the absence of
  a prompt within the read budget → `timeout`. A missing response for a signal that the PID
  scan already rejected must surface as unavailable, not as `0`.
- Pace requests: honour `STPTRQ` on ISO 9141-2/14230-4, and remember that a slow request
  response is a *quality* signal, not invisible latency.

### `disconnect()`

Stop polling, drain to the prompt, send `STPC` (close protocol; FRPM §8.6). Do not write
persistent state, do not send `ATZ` casually if it could clear the operator's stored settings —
`ATD`/`ATZ` are resets, not teardown.

### Contract gaps this exercise exposes

- There is no way to express "requested but unsupported by this vehicle" in
  `ConnectionCapabilities` beyond omitting the signal; the UI currently can only show the
  simulated five. `standard_obd_signals()` is a candidate list, not a support claim.
- `read_samples` has no per-signal staleness/age channel, yet on real hardware a PID that
  responds at 2 Hz must be distinguishable from one that has gone silent.
- No field for the negotiated protocol/ECU identity per request, although `Sample.ecu_id`
  partially covers it.

---

## 4. Read-only test sequence

**Stage A — bench, no vehicle.** The QSG documents the SX lighting its LED from the PC's USB
port alone, so this stage needs no car.

1. Record Windows build/arch, device manager tree, driver provider/version/date/INF, and USB
   identity (`VID_xxxx&PID_xxxx`, device instance path). Do not update the driver.
2. Note the assigned COM port, then close and reopen the port to see whether the number is
   stable across replug; record whether the banner reappears.
3. Terminal at 115200 8-N-1, no flow control, LF off. Capture the exact banner bytes and the
   prompt.
4. `ATI`, `AT@1`, `AT@2`, `STI`, `STSN`, `STDICPO`, `STDITPO`, `STDIX` — capture verbatim.
5. Probe, without writing anything: whether the documented defaults match (`ATE`/`ATL`/`ATH`/
   `ATS`/`ATR` state), whether `STPTO`/`STPTOT`/`STPTRQ`/`STPR`/`STPRS` are accepted or return
   `?`, and whether `ATST` (deprecated) still changes anything.
6. Confirm `STPRS`/`STPBRR` behaviour with no protocol open (`STPBRR` is documented to answer
   `?` when the protocol is AUTO).

**Stage B — stationary 2014 Scion xB (2AZ-FE), key-on engine-off, then idle.** Read-only Mode
01 only. Never Mode 04, never a write/actuator service, never `STPX`.

1. Open the session, run the PID scan, and record **every** responder and its bitmap.
2. Select the ECU with the largest supported set, as the QSG recommends.
3. Read RPM, coolant, intake air, throttle, speed. Sanity-check: engine-off RPM must be ~0;
   idle ~700–900 rpm; coolant must be plausible and rise slowly; speed 0.
4. Measure sustained per-PID rate and record it; compare against the simulator's 10 Hz target.
5. Pull the USB cable mid-acquisition: confirm the recovery path produces a gap and
   re-acquisition, never interpolated values.
6. Only if the SX is being retained for the Ford comparison: repeat the read-only scan on the
   2011 Flex **without** claiming MS-CAN/enhanced coverage — the SX cannot reach those buses.

---

## 5. Confirmed vs unknown

| Item | Status |
| --- | --- |
| SX device ID is **STN1130**; manufacturer classifies it **Obsolete** (no active firmware development) | **Confirmed** — FRPM §3.0–3.1, p. 6; product page |
| SX is discontinued / out of stock | **Confirmed** — product page |
| Default UART: **115200 bps, 8-N-1** for USB OBDLink adapters | **Confirmed** — FRPM §6.0, p. 8; corroborated by the OBD Solutions tutorial ("Configure the COM port for 115.2kbps") |
| Commands CR-terminated (0x0D); responses CR-terminated; `ATL1` adds 0x0A; `CR` repeats last command; firmware ignores 0x0A | **Confirmed** — FRPM §6.0 p. 8, §9.0 p. 54 |
| Always wait for the `>` prompt before the next command | **Confirmed** — FRPM §6.0, p. 8 |
| Startup banner is `ELM327 v1.4b` (the tutorial shows `ELM327 v1.3a` for an older unit) | **Confirmed as documented, version-dependent** — FRPM §6.0 p. 8 vs tutorial |
| `STI` returns `STN<device_id> vX.Y.Z`; `STDIX` dumps extended identity; `STSN` is the serial | **Confirmed** — FRPM §8.4, p. 31 |
| Default OBD request timeout **102 ms**; per-protocol transmit timeout 300/300/50 ms (J1850 PWM/VPW, ISO9141-2/14230-4, CAN); `STPTRQ` 56 ms on ISO 9141-2/14230-4 | **Confirmed** — FRPM §8.6, p. 35 |
| ELM327 command set is emulated; AT commands behave as on an ELM327 "with few exceptions"; AT/ST command support varies (supported/deprecated/not yet supported) and protocols are "not available in all devices" | **Confirmed** — FRPM §7.0 p. 9, §7.1 p. 9, §4.0 p. 6 |
| `ATST`, `ATDP`, `ATDPN`, `ATMA`, `ATAR`, `ATPC`, `ATSS`, `ATTP`, `ATRV`, `ATCV` are **deprecated** here | **Confirmed** — FRPM §7.1 Tables 1, 3, 5, pp. 9–10 |
| PID-level semantics are J1979/ELM327, not STN-specific; Mode $01 = live data | **Confirmed** — FRPM §10.0, p. 54; OBD Solutions "OBD Modes" |
| No Ford MS-CAN / GM SW-CAN on the SX | **Confirmed** — product page ("Consider… OBDLink MX+ if you need… Ford MS-CAN and GM SW-CAN"); FRPM §3.1, p. 6 |
| Multi-ECU response handling is required; one ECU at a time | **Confirmed** — SX Windows Quick Start, step 8 |
| Which firmware version the bench unit runs | **Unknown** — read via `STI` |
| USB VID/PID, device description, and whether Windows binds FTDI VCP vs `usbser.sys` by default | **Unknown** — read from Device Manager |
| Driver version/build and whether the port re-enumerates on replug | **Unknown** |
| Whether the banner/prompt appears on port open or only at power-up | **Unknown** — field test stage A step 3 |
| Which of the deprecated AT commands still function on the SX's frozen firmware | **Unknown** — must be probed, not assumed |
| Whether `STPTO`/`STPTOT`/`STPTRQ` are accepted by the SX firmware (FRPM is family-wide) | **Unknown** |
| Whether `ATSP`/`STWBR`/`ATPP` persistence survives power cycles (and whether we may use it at all read-only) | **Unknown** — avoid |
| Supported-PID bitmap for the 2014 xB 2AZ-FE, and its per-ECU split | **Unknown** — field test stage B |
| Achievable per-PID rate on the xB (CAN vs K-line), and error/retry counts | **Unknown** — measure |
| Latency timer / FTDI advanced settings needed for sub-100 ms polling | **Unknown** |
| Exact ELM327 datasheet wording for the OBD request format | **Not verified here** — the FRPM §10.0 delegates to it; the datasheet is PDF-only and could not be machine-read in this environment |
| FTDI's own driver documentation | **Not verified here** — `ftdichip.com/drivers/vcp-drivers/` returned HTTP 403 (bot challenge) to the fetch tool |
| Behaviour of ELM327 clones, and any VID/PID commonly attributed to the SX in forum posts | **Deliberately not used as evidence** |

---

## 6. Physical-test record template

Copy one block per session.

```
Session ID:
Date / time (local, UTC offset):
Operator:

--- Host ---
OS edition + build + arch:
USB host controller / hub used:
Driver: provider, version, date, INF name:
USB identity: VID_ , PID_ , device instance path:
COM port assigned: COM   Baud/format: 115200 8-N-1 (or measured)
Replug test: port number stable? yes/no   Banner on reopen? yes/no

--- Adapter ---
ATI (verbatim):
STI (verbatim):
AT@1 / AT@2 (verbatim):
STSN (verbatim):
STDICPO / STDITPO:
STDIX (verbatim, unparsed):
Firmware update state (as-found only; updated? no):
Accepted setup commands (verdict per command: OK / ? / STOPPED / NO DATA):
  ATE0:   ATL0:   ATH1:   ATS0:   ATR1:   ATSP0:
  STPTO:  STPTOT: STPTRQ: STPR:   STPRS:  STPCB:
  Deprecated probes: ATST: ATDP: ATMA:
Defaults observed before any change (echo/LF/headers/spaces/responses/timeout):

--- Vehicle ---
Vehicle + year + engine + VIN (redact before sharing):
Ignition state (key-on engine-off / idle):
Battery voltage (if available):
Protocol negotiated (STPRS verbatim):
Responders seen (count) and per-responder PID-00 bitmap (0100 raw):
  ECU A: 0100 =             0120 =             0140 =             0160 =
  ECU B: 0100 =             0120 =             0140 =             0160 =
ECU selected for acquisition + why:

--- Acquisition ---
Signals requested (PID list):
Raw response trace (verbatim, timestamped, one line per request/response):
Measured rate per PID (Hz), sustained over >= 30 s:
Value sanity checks (engine-off RPM; idle RPM; coolant rising; speed 0):
Error counts by type: NO DATA / ? / STOPPED / <RX ERROR / BUS BUSY / timeout / parse:
Cable-pull recovery observed (gap event shown? re-acquisition? values interpolated?):
Notes / anomalies:
```

---

## 7. Sources

Primary vendor documents (all read directly, 2026-10-02):

1. **OBDLink Family Reference and Programming Manual (FRPM), Rev F, 2025-08-29** —
   <https://www.scantool.net/scantool/downloads/682/obdlink_frpm_f.pdf> (canonical link
   <https://www.obdlink.com/frpm>). Cited sections: §3.0–3.2 (p. 6), §4.0 (p. 6), §6.0 (p. 8),
   §7.0–7.1 Tables 1–5 (pp. 9–10), §8.4 (pp. 30–31), §8.6 (p. 35), §9.0 (p. 54), §10.0 (p. 54),
   §11.0 (pp. 54–58). Appears as "OBDLink FRPMF" in the page footer; 78 pages.
2. **OBDLink SX product page** — <https://www.obdlink.com/products/obdlink-sx/> (discontinued
   notice, protocol list, "USB 2.0", Windows Vista+, MS-CAN/SW-CAN exclusion).
3. **OBDLink SX support page** — <https://www.obdlink.com/support/sxusb/> (Setup Guide, Quick
   Start Guide, FTDI Virtual COM Port Driver, firmware download).
4. **OBDLink SX Windows Quick Start Guide** — <https://www.obdlink.com/windows_qsg_sx/>
   (LED states, driver install, Auto-Detect over COM ports *and* baud rates, multi-ECU note).
5. **OBDLink: Get Started with Your OBDLink Adapter (Windows)** —
   <https://support.obdlink.com/support/solutions/articles/43000727094-get-started-with-your-obdlink-adapter-windows->
   (Communication Type = USB for SX/EX; one app at a time; STN Firmware Updater).
6. **OBDLink: Update OBDLink Adapter Firmware** —
   <https://support.obdlink.com/support/solutions/articles/43000705180-update-obdlink-adapter-firmware>
   (SX firmware paths per OS; USB OTG requirement on Android).
7. **OBDLink SX firmware updates** — <https://www.scantool.net/downloads/updates/obdlink_sx/>
   (STN Firmware Updater usage).
8. **OBD Solutions: Reading Real-Time Data** —
   <https://www.obdsol.com/knowledgebase/obd-software-development/reading-real-time-data/>
   (115.2 kbps COM port, protocol set/display, `010C`→`41 0C 0F A0`, PID conversions).
9. **OBD Solutions: Getting Started** —
   <https://www.obdsol.com/knowledgebase/obd-software-development/getting-started/>
   (J1979 as the governing standard; FRPM describes the STN1100 family command set; ELM327
   datasheet describes the emulated command set).
10. **OBD Solutions: OBD Modes** —
    <https://www.obdsol.com/knowledgebase/obd-software-development/obd-services-modes/>
    (Mode $01 = Request Live Data).
11. **ScanTool.net: Switching Communication Baud Rate** —
    <https://www.scantool.net/blog/switching-communication-baud-rate/> (`STSBR`/`STWBR`,
    `ATPP 0C`, `STI` example `STN1101 v2.1.0`, 38400→115200 default change).
12. **ScanTool.net Product Documentation index** —
    <https://www.scantool.net/scantool/downloads/product-documentation/> (revision/date of the
    FRPM and related datasheets; STN Bootloader firmware-update specification;
    "Creating a Communication Log").

Referenced but **not verified in this environment** (PDF-only or bot-blocked):
ELM327 datasheet <https://www.elmelectronics.com/wp-content/uploads/2017/01/ELM327DS.pdf>
(delegated to by FRPM §10.0) and the FTDI VCP driver page
<https://ftdichip.com/drivers/vcp-drivers/> (HTTP 403 to the fetch tool). SAE J1979 / J1979-2 /
J1979DA are paywalled; FRPM §10.0 names them as the governing standards.

## 8. Explicit non-claims

- No adapter, driver, port, firmware, ECU, or vehicle was touched; nothing in this brief is a
  test result.
- No compatibility or baud rate is inferred from ELM327 clone behaviour.
- No Ford MS-CAN/GM SW-CAN capability is attributed to the SX.
- Because the SX is `Obsolete` and its firmware is frozen, every command and default above
  remains a *hypothesis about this unit* until Stage A records the actual replies.
