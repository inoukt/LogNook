# Xtool A30 protocol research archive

These source notes were copied from `X:\p1` on 2026-10-04 for future LogNook protocol work:

- [A30 transport and protocol export](A30_PROTOCOL_EXPORT.md) — byte layouts and observed request/reply shapes.
- [2014 Scion xB session analysis](A30_XB_20261004_ANALYSIS.md) — vehicle-specific capture narrative and raw response examples.

The source-file SHA-256 hashes are `AD079B1C1851A6D23F08C3FE20FE4217561C931A0227006124BCAB5A94FBC7BB` and `4AE89D93D0F237533C476B72E354EBE3DD551AFED7F875491BB3929865326A4D`, respectively. The 3.2 MB `.cfa` remains at `X:\p1\btsnoop_hci_20261004130-.cfa` and is not checked into this repository.

## LogNook implementation boundary

The A30 SPP/RFCOMM and dongle framing are vendor-specific. Reuse only vehicle-level request/response evidence when it matches LogNook's OBDLink SX or J2534 path; this archive does not define an A30 adapter implementation. Raw replies establish ECU response, not decoder correctness or vehicle validation.

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

Thus the capture advertises 11 actual TIDs: `01`, `02`, `21`, `35`, `3D`, `42`, and `A1`–`A5`. It queried only `35`, `A2`, and `A5`. The captured results are in the analysis file; MID meanings and UAS scaling are not known. LogNook does not currently implement Mode 06.

The A30 response times for these nine Mode 06 exchanges ranged from about 96 ms to 532 ms. Treat these as measurements of that A30 setup, not defaults for the SX or J2534 adapters.
