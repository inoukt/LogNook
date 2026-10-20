# Original Tactrix OpenPort 2 — Ford diagnostic confirmation

Date: 2026-10-15 (America/Toronto).

## Evidence and scope

The owner connected the OpenPort 2 to the Ford and operated LogNook's normal
Diagnostics controls. This report records the output supplied by the owner in
the current chat; it is not an independent agent-operated vehicle test. Vehicle
context is the project's 2011 Ford Flex. Ignition/engine state was not freshly
reconfirmed for this specific pasted read.

The app used the original registered Tactrix provider, previously identified as
`Tactrix Inc. - OpenPort 2.0 J2534 ISO/CAN/VPW/PWM`. Earlier read-only Windows
inventory showed the Tactrix `openport` driver, version `1.0.0.4227`, device
problem code zero, and the x86 `op20pt32.dll` provider. This confirmation did not
repeat that inventory.

Local app: `target/release/lognook-desktop.exe` in the
`C:/Users/inouk/LogNook-can-event-freeze-frame` checkout. Branch:
`codex/can-event-freeze-frame`, based on `eda608b3f0c65572e6daac1a76556f4ea5343b23`
with uncommitted feature and parser fixes. The i686 release J2534 helper includes
receive-start notification filtering and padded ISO-TP flow-control filters.

## Owner-reported results

Functional request route: `7DF`; responding ECU: `7E8`; ISO15765, 11-bit,
500 kbit/s.

| Request | Outcome | Decoded result |
|---|---|---|
| `03` | Positive | One stored DTC: `P0605` |
| `07` | Positive | One pending DTC: `P0605` |
| `0A` | Positive | Two permanent DTCs: `P0605`, `P0442` |
| `09 02` | Positive | Full VIN displayed in the app; omitted from repository documentation |
| `09 04` | Positive | Calibration ID `DMJKVTA.H32` |
| `09 0A` | Positive | ECU name `PCM-PowertrainCtrl` |
| `06 00` | Unsupported | Not sent: OpenPort Mode 06 is not implemented |

The UI reported 1–3 ms for the positive reads. These displayed durations are
not an independent bus timing measurement or a sampling-rate benchmark.

Raw replies supplied by the owner, excluding the full VIN:

```text
7E8 43 01 06 05
7E8 47 01 06 05
7E8 4A 02 06 05 04 42
7E8 49 04 01 44 4D 4A 4B 56 54 41 2E 48 33 32 00 00 00 00 00
7E8 49 0A 01 50 43 4D 00 2D 50 6F 77 65 72 74 72 61 69 6E 43 74 72 6C 00
```

## Confirmed fixes and remaining scope

- Tactrix `START_OF_MESSAGE` notifications no longer appear as malformed ECU replies.
- Padded flow-control filters allow the observed VIN and calibration replies to complete.
- The captured 20-byte ECU-name layout is decoded with separate acronym/description padding; raw reply bytes are preserved.
- Software verification before this live confirmation passed 43 worker tests and 263 core library tests; release helper and desktop builds succeeded.
- This run does not validate OpenPort Mode 06 or OEM Mode 21/22 support, other vehicles/providers, sustained logging rates, or physical DTC clearing. No clear was requested in this confirmation.
- The decoder and transport fixes serve the shared desktop/MCP path. MCP invocation itself was not physically exercised in this run.

No source commit or push was made for this confirmation.
