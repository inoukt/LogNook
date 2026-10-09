# Xtool A30 protocol research archive

These source notes were copied from `X:\p1` on 2026-10-04 for future LogNook protocol work:

- [A30 transport and protocol export](A30_PROTOCOL_EXPORT.md) — byte layouts and observed request/reply shapes.
- [2014 Scion xB session analysis](A30_XB_20261004_ANALYSIS.md) — vehicle-specific capture narrative and raw response examples.

The source-file SHA-256 hashes are `AD079B1C1851A6D23F08C3FE20FE4217561C931A0227006124BCAB5A94FBC7BB` and `4AE89D93D0F237533C476B72E354EBE3DD551AFED7F875491BB3929865326A4D`, respectively. The 3.2 MB `.cfa` remains at `X:\p1\btsnoop_hci_20261004130-.cfa` and is not checked into this repository.

## LogNook implementation status

LogNook now has a Windows Classic Bluetooth SPP transport, an A30/Xtool binary session, and a Mode 01 `HardwareAdapter`. It uses the standard SPP service UUID and Windows SDP channel resolution, then sends the captured SETUP/STATUS/poll/config sequence before reading vehicle data. Pair the dongle in Windows Bluetooth settings first; LogNook lists paired devices and does not require a virtual COM port.

The OEM research panel exposes one-shot tests for Mode 01 RPM, Mode 02 freeze-frame RPM, Mode 03 stored DTCs, Mode 06 supported-TID bitmap pages and captured Scion TIDs 35/A2/A5, Mode 07 pending DTCs, Mode 09 VIN, and Mode 0A permanent DTCs. Service 21 and 22 candidates are issued only from the read-only v0.3 catalog, with their exact source request and ECU route. Finite decoded numeric values enter the shared sample pipeline and graph; Mode 06 results use the explicit `raw count` unit. Research/vehicle-validation metadata remains unchanged. Raw A30 test results append to app-data `oem-validation/a30-read-tests.jsonl`; candidate outcomes append to `oem-validation/oem-candidate-tries.jsonl`.

Continuous OEM polling now shares a 50 ms request-start target (up to 20 starts/s) across the adapters that implement it: OBDLink SX, ELM327, and A30. It does not add a fixed delay after a completed reply. A30 returns as soon as the current outer sequence contains a complete response from the expected ECU, while retaining its 400 ms silence/mismatch timeout. No response, decode failure, or out-of-range values back the interval off up to one second; good replies ease it back down. The connected A30 Mode 01 path also matches the identified ECU response early. A30's captured 20 requests/s is a reference target; LogNook's actual rate depends on adapter and ECU latency. OpenPort 2 currently has no continuous service 21/22 request path.

The captured `.cfa` is not stored in the repository. Tests use small byte-exact fixtures transcribed from the checked protocol export and Scion xB analysis. No physical Bluetooth A30 handshake has yet been verified by this checkout. Raw replies establish ECU response, not decoder correctness or vehicle validation.

The capture contains state-changing service 30 actuator commands and an unknown multi-frame write in addition to reads. Their presence documents what the other tool sent; it is not authorization to reproduce them. LogNook's root `AGENTS.md` safety boundary still applies.

## Verified Scion xB capture notes

The correctly decoded stream is HCI handle `0x13`, RFCOMM DLCI 12. It contains 993 host frames and 1,979 dongle frames, with zero A30 XOR mismatches. It identifies VIN `JTLZE4FE1EJ049314`, ECU text `AZE151 2AZFE`, and engine route `0x7E0 → 0x7E8`.

The Mode 21 `21 01` reply is a 28-byte PDU (`61 01` plus 26 data bytes), observed 424 times. Its field map remains unresolved. LogNook's one-shot accepts this PDU size and the prior 29-byte shape, while leaving its five catalog-derived channels unvalidated.

Mode 06 bitmap continuation bits must not be counted as test IDs. The verified replies are:

| Request | Response | Meaning |
|---|---|---|
| `06 00` | `46 00 C0 00 00 01` | TIDs `01`, `02`; continuation at `20` |
| `06 20` | `46 20 80 00 08 09` | TIDs `21`, `35`, `3D`; continuation at `40` |
| `06 40` | `46 40 40 00 00 01` | TID `42`; continuation at `60` |
| `06 60` | `46 60 00 00 00 01` | Continuation at `80` |
| `06 80` | `46 80 00 00 00 01` | Continuation at `A0` |
| `06 A0` | `46 A0 F8 00 00 00` | TIDs `A1`–`A5` |

Thus the capture advertises 11 actual TIDs: `01`, `02`, `21`, `35`, `3D`, `42`, and `A1`–`A5`. It queried only `35`, `A2`, and `A5`. The captured results are in the analysis file; MID meanings and UAS scaling are not known. LogNook exposes the captured bitmap pages and Scion TIDs 35, A2, and A5 as one-shot reads. Their numeric values are graphed only as raw counts; no unknown scaling is inferred.

The A30 response times for these nine Mode 06 exchanges ranged from about 96 ms to 532 ms. Treat these as measurements of that A30 setup, not defaults for the SX or J2534 adapters.
