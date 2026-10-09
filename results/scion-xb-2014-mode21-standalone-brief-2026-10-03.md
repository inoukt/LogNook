# 2014 Scion xB Mode 21 standalone research brief

**Prepared:** 2026-10-03  
**Purpose:** Carry the Mode 21 findings and next-test plan into LogNook’s main project thread.  
**Scope:** Read-only research and offline reference decoding. No LogNook source was changed and no command was sent to the vehicle or SX during this research.

## Finding

The earlier direct tests sent Toyota Service 22 requests. The ECU replied `7F 22 11` at the tested 7E0 route, which rejects those Service 22 requests only. It does not test or reject Toyota Service 21.

The current OBDb Scion-xB profile contains Service 21 definitions. Its profile commit, [`d77a1cd`](https://github.com/OBDb/Scion-xB/commit/d77a1cdde8706f0d14386096404b67bc6dc4669c), added 213 Toyota-family candidate commands, including 107 Service 21 commands. The commit describes them as debug candidates and says the available xB captures had requested only Service 01, so these Service 21 definitions are not confirmed live results for the owner’s 2014 xB.

## Best first request

Try the profile’s engine data block **Service 21 PID 01 (`21 01`)** first:

- CAN request header: `7E0`
- Expected ECU response: `7E8`
- Positive service and PID after ISO-TP reassembly: `61 01`
- Profile response data: 27 bytes after `61 01`
- Common fields in the candidate block:

| Field | Data offset after `61 01` | Candidate decoding |
|---|---:|---|
| Intake temperature | 6 | byte − 40 °C |
| Coolant temperature | 8 | byte − 40 °C |
| Engine speed | 9–10 | big-endian 16-bit value ÷ 4 rpm |
| Vehicle speed | 11 | byte in km/h |
| Throttle position | 14 | byte × 100 ÷ 255 % |

The initial transaction, using LogNook’s established SX serial transport and the detected CAN 11/500 protocol, is `STPX H:7E0, D:2101, T:250` followed by the normal carriage return. Do not set an assumed one-frame response limit: reference replies span five CAN frames. Capture the full adapter response through its prompt, check the ISO-TP length and sequence, then compare the five decoded values with a paired Mode 01 read.

## Reference evidence and limits

I reassembled one public 2010 and one 2011 Camry `21 01` response offline. Each had a 29-byte ISO-TP payload (`61 01` plus 27 data bytes); the five values above matched the corresponding fixture expectations in both cases. These captures validate the proposed decoder against those reference frames, not against the Scion.

The OBDb commit attributes a similar layout to a 2AZ-FE Camry. The fixture captures do not include an ECM calibration ID or independently verified engine identity, and Toyota’s North American 2010 Camry release says its new 2.5-litre engine replaced the earlier 2.4-litre engine. Treat the claimed engine match as unconfirmed. The live Scion comparison is decisive.

Sources:

- [Scion-xB profile](https://raw.githubusercontent.com/OBDb/Scion-xB/d77a1cdde8706f0d14386096404b67bc6dc4669c/signalsets/v3/default.json)
- [OBDb profile seed commit and its evidence notes](https://github.com/OBDb/Scion-xB/commit/d77a1cdde8706f0d14386096404b67bc6dc4669c)
- [2010 Camry reference fixture](https://raw.githubusercontent.com/OBDb/Toyota-Camry/main/tests/test_cases/2010/commands/7E0.7E8.2101%7Cf%3D-2011.yaml)
- [2011 Camry reference fixture](https://raw.githubusercontent.com/OBDb/Toyota-Camry/main/tests/test_cases/2011/commands/7E0.7E8.2101%7Cf%3D-2011.yaml)
- [Toyota Canada: 2010 Camry engine change](https://media.toyota.ca/en/whats-new/2009/whats-new-2010-toyota-camry.html)
- [OBDLink Family Reference and Programming Manual: STPX](https://www.scantool.net/scantool/downloads/678/obdlink_frpm_e.pdf)

## What LogNook needs

The shared `HardwareAdapter` contract can already report signals with an 8-bit service and PID and return normal samples. The current SX implementation only accepts the standard Mode 01 catalog, emits Service 01 requests, and uses a Mode 01 decoder. Adding a catalog row alone will not enable Mode 21.

The smallest implementation after the vehicle read succeeds is a bounded Service 21 block decoder that reuses the existing SX transport and ISO-TP assembly, maps selected offsets from one `21 01` response, and emits ordinary timestamped samples. Mode 01 support bitmaps cannot prove Service 21 support. Add the actual Scion response as a fixture, then test positive decoding, truncated or out-of-order frames, negative replies, and value comparisons. Only Scion signals confirmed by the live response should become selectable.

Other Scion-xB profile candidates, pending a successful first read, include `21 E4` injector pulse width, `21 3C` injection data, `21 45` misfire data, `21 54` per-cylinder/average RPM, and `21 82` transmission temperatures. These have not been tested on the owner’s xB.

## Physical test status and next step

At the time of this brief, the owner had disconnected the vehicle/adapter. No Service 21 request, live sample, session change, or setting change was made. The next test is one `21 01` request with the vehicle stationary in Park, parking brake set, ignition on, engine off, and all other clients closed. Stop and save the raw reply if it is negative, incomplete, or malformed; do not scan identifiers or add undocumented setup commands. If values decode plausibly, repeat paired Mode 01/21 reads briefly to measure update rate and gaps. Engine-idle behavior can be checked later only while the owner runs the engine outdoors.

See also the larger [Scion xB validation report](scion-xb-2014-live-app-validation-2026-10-03.md) and [pinned Mode 21 research data](scion-xb-2014-mode21-research-2026-10-03.json).
