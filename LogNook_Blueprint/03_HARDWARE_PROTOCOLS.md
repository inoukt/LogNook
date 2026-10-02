# Hardware and diagnostic protocol plan

## Interface order (all four targeted for first supported product release)
1. **Generic ELM327 USB**: enumerate safe serial ports, probe ELM-compatible commands and AT identity; basic reset/echo/linefeed/header configuration; check actual firmware command behavior, clone inconsistencies, available bus protocol, supported PID pages. Initially target standard CAN Mode 01 read-only; add K-line and J1850 based on measured hardware/vehicle compatibility.
2. **OBDLink SX USB**: FTDI virtual COM and STN1130-type device identification, firmware awareness and separate ST command fast path. SX manufacturer claims legislated OBD protocols including CAN, KWP, ISO9141 and J1850; do not claim Ford MS-CAN/GM SW-CAN on SX hardware.
3. **RH-02/CANable**: explicitly detect candleLight/`gs_usb` vs SLCAN firmware; show VID/PID only as detection hints (clones vary), verify capability and supported bitrate. Configure 11/29-bit receive, filters if available, high-load buffering, silent/passive mode only where firmware truly supports it, raw timestamp quality. Guide WinUSB driver setup only if enumeration requires it. Linux `SocketCAN` later. Physical breakout wiring, bus bitrates and 120-ohm termination must be technician-confirmed.
4. **Tactrix OpenPort 2.0**: discover J2534 v04.04 and v05.00 Windows registry entries when applicable; vendor DLL path, 32-bit registry view, and architecture-matched worker; validate CAN/ISO15765 and K-line in separate tests, plus hotplug and driver versions. Production preference is the official vendor driver; never replace it just to achieve a portable install. On 2026-10-02 the user chose to retain the active OPTA WinUSB binding and authorized a bounded diagnostic review of its matching DLL. That exception does not make OPTA the production choice: CAN connected, but reliable raw J2534 frames were not verified because the returned message combined frame data. See the [physical OpenPort 2 report](../docs/hardware/2026-10-02-openport2.md). Tactrix describes OpenPort 2.0 as 12-V hardware; do not promise heavy-duty 24-V use.

## Vehicle protocol phases
- **A**: Generic SAE J1979 / Mode 01 supported-PID scan, selected emissions PID reading, source-ECU attribution, calculated units; VIN/Mode 09 where supported, robust multi-response handling.
- **B**: ISO-TP (ISO 15765-2) addressed sessions, UDS read-only DID 0x22 and metadata-driven decoder, session/response-pending timing, unsolicited responses, physical and functional addressing. OBD-on-UDS J1979-2 distinct from arbitrary manufacturer UDS; validate against current standards.
- **C**: KWP2000 and supported K-line variants; read-only enhanced OEM profile packs, limited to documented definitions/authorized access.
- **D**: DBC raw CAN decoding, raw frame capture and filtering; J1939 PGN/SPN plus DM views on hardware electrically suited to the target vehicle, not a blanket 24-V promise.
- **E**: Newer ZEV standard and DoIP with new compatible physical hardware; optional ODX/PDX and A2L imports if legal source files are supplied.

## Coverage is three separate claims
**Physical connection** != **protocol transport** != **decoded OEM content**. Display three checks in vehicle profiles. Generic emissions PIDs reach many brands but do not imply unrestricted access to ABS, airbag, transmission, BCM, hybrid battery, every OEM measurement or security gateway. Address protected services only with legitimate access and explicit permission, never guessed unlock secrets.

## Safe acquisition defaults
Read-only by default. No flashing, coding, clearing codes or actuator control in acquisition MVP. Raw CAN starts listen-only when confirmed supported and selectable; warn if filter support is host-side only. User must explicitly opt into sending custom frames. Do not auto-scan a live bus by spraying diagnostic requests across guessed identifiers.

## Connection negotiation
Keep a per-adapter safe capability cache keyed by hardware identity and firmware; keep protocol hints per vehicle/ECU but reverify each vehicle session. A priority scheduler owns requests so separate data channels cannot silently compete. Any unplug/replug generates a gap event and resync, not fictitious interpolated values.

## Real hardware questions for POC
Determine exact dongle firmware revisions, Windows 10/11 drivers and x86 J2534 DLL version; test RH-02 actual candleLight firmware and supported silent mode; verify STN extended commands on SX; acquire both a reliable ELM and a problematic clone; test multi-PID grouping on a real ECU; record observed rates and error counts by device, protocol and selected channels. None of these are established merely by vendor specs.
