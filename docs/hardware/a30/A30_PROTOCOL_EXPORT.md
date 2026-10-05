# Xtool A30 PRO (A30M) — complete dongle protocol export

One self-contained file: everything this repository knows about the A30 dongle protocol as of
**2026-10-04**, written so a different app can be built against it without reading the Kotlin sources.
It covers the phone↔dongle Bluetooth layer, the record/relay layer that carries UDS/J1979 traffic, and
the CAN framing underneath it (1004's key finding: the `obj`/`slot` bytes are a CAN id and an ISO-TP
PCI), plus every byte-exact request/reply shape confirmed by a capture or verified in a working client.

Companion material in this repo (not required to use this file): `A30PROTOCOL.md` (the working spec,
same facts + archaeology), `A30_XB_20261004_ANALYSIS.md` (the session walk-through behind every 1004
fact — timeline, activity-by-activity tables, evidence), `A30_dongle_reference.md` (decompile +
first-capture notes), `brand_profiles.contract.md` (the brand-behavior interchange format), `a30poc/`
(a runnable Python client and its self-test), `a30probe/` (the Android app built from all of this).

## 0. Evidence grades used throughout

Every statement carries one of these, so a reader knows what has been proven vs. merely built here:

| Grade | Meaning |
|---|---|
| **[CAPTURED 0915]** | Byte-verified in `btsnoop_hci_202609151523.cfa` — official app, classic Ford: connect → VIN read → DTC read → DTC clear. |
| **[CAPTURED 0921]** | Byte-verified in `btsnoop_hci_202609210050.cfa` — official app a second day (same bytes), plus our first client's checksum-less sessions. |
| **[CAPTURED 0925]** | Byte-verified in `btsnoop_hci_202609251529 ford multiple module and some live datas.cfa` — official app, Ford 6 modules, live mode-22 DID streaming (3672 frames). |
| **[CAPTURED 1004]** | Byte-verified in `btsnoop_hci_20261004130-.cfa` — Scion xB (VIN `JTLZE4FE1EJ049314`), a full Toyota ECM session: **mode-21 live data**, a **mode-01 full PID scan**, **mode 06** monitor tests, a **service-30 actuation sweep** — and the first mode-21 traffic ever captured. Same dongle as the earlier captures (identity blob matches). **All 1979 dongle frames and all 993 host frames carry valid XORs.** |
| **[BUILT]** | Implemented in `a30probe/` and verified by the off-device suite (`tools/check_transcript.sh`, 731+ checks; `python -m a30poc.selftest`) and/or live runs. |
| **[UNCLEAR]** | Seen but not understood, or untested. §14 lists these with the exact open question. |

The four captures are ground truth. When this file says "the capture", it means the 0915/0921/0925
captures unless marked 1004. The 1004 capture is separate evidence and is called out wherever it
differs (it is a different diagnostic app version, and it shows shapes no earlier capture contains).

---

## 1. Transport

| Item | Value | Evidence |
|---|---|---|
| Link | Classic Bluetooth **SPP (RFCOMM)**, insecure socket, UUID `00001101-0000-1000-8000-00805F9B34FB` | [CAPTURED] all captures; used live |
| Bonding | The device must already be **bonded** in Android; the dongle requests the stored link key (`Authentication_Requested`, link-key reply, encryption enable). No app-level auth. | [CAPTURED] both directions of HCI |
| Device name | Bluetooth name is **`A30M-…`** (e.g. `A30M-111948` in the 1004 capture); discovery matches the prefix | [CAPTURED 1004] |
| RFCOMM parameters | DLCI 12 is the data channel; credit-based flow control, initial credits 7, max frame size 990 | [CAPTURED] PN negotiation |
| **RFCOMM credit octet (parser trap)** | When a UIH frame is sent with the **PF bit set**, one **credit octet sits between the length octet(s) and the information field**; it is *not* counted by the RFCOMM length. In the 0915/0921 captures only the first UIH carries it; in 0925 (and 1004) **every** UIH carries PF+credit. Strip the credit if `ctrl & 0x10`, or the stream shifts and fakes XOR failures. | [CAPTURED 0925] fixed 1700 phantom mismatches |
| Multi-frame payloads | One L2CAP payload can carry **several** RFCOMM UIH frames — walk all of them, don't read just the first. | [CAPTURED 0925] |
| Chunking | Dongle-side L2CAP payloads observed 6–43 B; an A30 frame can span chunks and must be reassembled by concatenation before decoding. | [CAPTURED 0925] |
| BLE alternative (other Xtool models) | GATT service: any UUID ending `0000FEE7-0000-1000-8000-00805F9B34FB`; write `000036F5-…`; notify `000036F6-…` (+ CCCD `2902` = `01 00`); 20-byte writes, autoConnect off, 10 s timeout. **Not used by the A30 PRO in any capture** — SPP only. | decompile; [CAPTURED] no ATT traffic |
| App logging | The official app logs every frame itself under tag `XTOOL debug`, lines `send->` / `read->` + hex — useful when comparing | decompile |

There is **no login, token, rolling code or challenge/response** on this link: the host's fixed
frames are byte-identical across capture days, the only per-frame variation is the sequence counter,
and the 16-byte identity blob the dongle sends is static. Android's HCI snoop log is how all captures
here were made.

---

## 2. A30 wire frame (both directions)

```
host  -> dongle:  55 AA | seq(u16 BE) | len(u16 BE) | cmd(u16 BE) | data(len) | xor
dongle -> host:  AA 55 | seq(u16 BE) | len(u16 BE) | cmd(u16 BE) | data(len) | xor
```

| Field | Notes | Evidence |
|---|---|---|
| Magic | Direction-specific: `55 AA` = to dongle, `AA 55` = to host. | [CAPTURED] |
| seq | u16 big-endian; the host increments per frame (starts at 1); the dongle has its own counter and **echoes** the host's seq on replies to that frame (`aa 55 00 0b …` answers `55 aa 00 0b …`). It is not a challenge. | [CAPTURED] |
| len | u16 BE, **payload length only** (not the checksum). | [CAPTURED] |
| cmd | u16 BE command word, §3. | [CAPTURED] |
| **xor** | **Required.** XOR of every byte before it — both magic bytes, seq, len, cmd and all data. 714/714 official frames across 0915/0921, 3672/3672 in 0925, 1979/1979 dongle frames in 1004. A frame without it is not answered with an error: the dongle reads the *next* frame's magic as the missing checksum and **never resynchronizes** (our first live build: 90 frames out, permanent silence). | [CAPTURED] all |
| Frame size | `8 + len + 1`, max len guard 2048. | [BUILT] |
| Escaping | None. Parse strictly by `len`, then verify the XOR; on mismatch you have lost byte sync. | [CAPTURED] |

**Boundary hazard**: a frame whose checksum byte is `0xAA` followed by the next frame's `55 AA` forms
the reverse magic across the boundary. Scan for one direction's magic only (never both in the same
stream). [CAPTURED 0925: 1/1225 phantom frame]

Byte-exact example — the official app's first SETUP frame (seq 1, the dongle answered it):

```
55 aa 00 01 00 07 ff f8 01 00 04 67 02 01 00 9f
                                             ^^ XOR of all preceding bytes
```

### 2.1 The record layer carries raw CAN frames (1004 — what obj / slot / trailer really are)

The 13 bytes after a record's 6-byte preamble are a classic-CAN frame plus its length:

```
01 00 10 61 04 00 | 0b | 08 | 07 e0 | 02 21 01 00 00 00 00 00 | ff
   fixed preamble   len  DLC   CAN id   8 CAN data bytes          trailer (outside the frame)
```

* `0b` = u8 length of everything after it in the record = 11 (`DLC + id + 8 data`). It is a **length
  byte**, not a service marker; what follows is `08` (DLC = 8) and the CAN id high byte (`07`).
* The CAN id is the 11-bit OBD id, big-endian: `07 e0` → 0x7E0 (engine), `07 df` → 0x7DF
  (functional OBD), `07 c0` → 0x7C0 (the second module 1004 talked to), `07 50` → 0x750 (probed
  once). What the rest of this file calls **`obj` is the low byte of that CAN id** (§7).
* The byte this file used to call `slot` is simply **the first CAN data byte** — and in every captured
  request but one it is the ISO-TP PCI: `01`–`07` = single frame of that many bytes (`01 3e` = payload
  `3e`; `02 21 01` = `21 01`; `03 22 d1 00` = `22 d1 00`; `04 30 01 00 8a` = `30 01 00 8a`), while
  `30 00 00 …` is a flow-control frame (§5.2). The exception is the unanswered `50 c8` probe (§7.1),
  whose `c8` first byte is not an ISO-TP PCI.
* The `trailer` byte sits **outside** the CAN frame — it is a dongle directive (stream/timeout hint),
  not vehicle data; §7.3 lists the observed values.
* Dongle replies carry the responding side's CAN id: `07 e8` = 0x7E8 (the ECM — it also answers every
  OBD request sent to the functional 0x7DF) and `07 c8` = 0x7C8 (the module polled as `c0`). Physical
  request id + 8 = response id (7E0→7E8, 7C0→7C8).

This one reading collapses several earlier mysteries at once: the "slot selects the service family"
rule, the "`0b 08 07` marker", and the per-service trailer constants are all consequences of the
record layer relaying CAN. Consistent with every earlier capture; first written out in 1004.

---

## 3. Command words (cmd u16 BE)

| cmd | Name | Dir | Meaning / payload | Evidence |
|---|---|---|---|---|
| `0xFFF8` | SETUP | h→d, d→h | Bring-up, and the dongle's own descriptor announcements. Host payloads `01 00 04 67 02 01 00` (P_SETUP) and `01 00 04 60 05 07 01` (object-5 variant). Dongle payloads `01 00 04 06 10 05 10` / `01 00 04 06 01 07 80`. | [CAPTURED] |
| `0xFFF9` | STATUS | h→d | `01 00 03 60 06 00`, sent 3× ~2 s apart during open. Each answered with the bare `00 00 02 00 00`. | [CAPTURED] |
| `0xFFFA` | POLL | h→d, d→h | Host heartbeat/object poll `01 00 02 60 <obj>`; dongle answers bare `00 00 02 00 00`, or streams data when the object is right (§4). | [CAPTURED] |
| `0xFFF7` | CONFIG | h→d | `01 00 05 60 01 00 8c 31`. Part of the open burst. | [CAPTURED] |
| `0xFFF4` | TIMING | h→d | `01 00 08 60 02 01 04 03 12 00 08` in 0915/0921/1004; `01 00 08 60 0c 00 00 ff ff ff ff` seen in 1004 setup. | [CAPTURED] |
| `0xFFF2` | TIMEOUT | h→d | Official: `01 00 0a 60 03 00 3c 00 64 00 05 00 00`; long-read variant `01 00 0a 60 03 00 96 01 f4 00 01 00 00`; 1004 used `01 00 0a 60 03 00 78 03 84 00 0a 00 00` / `… 02 58 09 c4 00 0a 00 01`. | [CAPTURED] |
| `0xFFF5` | DESCR | h→d | `01 00 07 60 08 01 07 e8 07 ff` (also `07 00 07 00` variant). Part of open and VIN fallbacks. | [CAPTURED] |
| `0xFFE8` | BLOCK | h→d | Stream setup: **23-byte** payload (§5.2). The CAN data it carries is the **flow-control frame `30 00 00 …`** sent to `<obj>` — this is what keeps a multi-frame answer (VIN, live block) alive while the dongle re-sends it. | [CAPTURED] |
| `0xFFE6` | BLOCK variant | h→d | **New in 1004**: 25-byte payload `01 00 16 60 09 05 01 10 0d 88 18 da 00` + record `e0 30 00 04 00 00 00 00 00 fc` + `00 00`. Same family as `FFE8` with a longer preamble and an extra `04` byte. | [UNCLEAR] [CAPTURED 1004] |
| `0xFFEC` | RECORD | h→d, d→h | The workhorse request record, **19-byte** payload (§5.1). Also used *by the dongle* to send its static identity blob (§6.4). | [CAPTURED] |
| `0xFFF1` | NOTIFY | d→h | Length-delimited reply record; carries UDS/J1979 answers (§6.1). | [CAPTURED] |
| `0xFFE4` | block reply, **2 records** | d→h | `<count=02>` + two 13-byte records (§6.2). Previously "BLOCK2, role not isolated". | [CAPTURED 1004] resolved |
| `0xFFD7` | block reply, **3 records** | d→h | `<count=03>` + three records. It carried the VIN in every capture because that answer needs 3 records — it is **not** VIN-specific. | [CAPTURED] resolved |
| `0xFFCA` | block reply, **4 records** | d→h | `<count=04>` + four records. Long DIDs, mode-03 lists, 1004's 21-byte mode-21 answer. | [CAPTURED] resolved |
| `0xFFBD` | block reply, **5 records** | d→h | `<count=05>` + five records. **1004**: the 28-byte mode-21 `61 01` live block (424 of them). | [CAPTURED 1004] |
| `0xFF6F` | block reply, **11 records** | d→h | `<count=0b>` + eleven records. **1004**: the `e0 a8 02` answers. One block can hold **several complete answers** — split on every PCI `10`. | [CAPTURED 1004] resolved |
| `0xFF55` | block reply, **13 records** | d→h | `<count=0d>` + thirteen records. **1004**: the `c8 a8 02` answer (a 62-byte message plus more). | [CAPTURED 1004] resolved |
| `0xFF4F` | long-record block | d→h | `<count=03>` + three **long records** (§6.3). **1004**: the `c0 a8 01` answer (176 B). | [CAPTURED 1004] resolved |
| `0xFFFB` | ACK | d→h | `01 00 01 00` (and `01 00 01 01` after a poll in 0925/1004). The dongle's bare heartbeat ack — **not** proof of a live diagnostic session. | [CAPTURED] |
| `0xFFFC` | END | d→h | `01 ff ff`, ends a block/sub-session (28× at 0925 teardown). Semantics unsettled. | [CAPTURED] |
| `0xFFEA` | timer record | h→d | **1004**: `01 00 12 61 04 00` + `0d` + `88 18 da <00\|5a>` + `<obj> <8 CAN bytes>` + `ff` — sent 4× while installing the `3e 00` keepalive (interval byte `5a` once). The dongle then repeats the record on its own timer, which is why the app stops sending 3E manually. `18 da` is the UDS-on-CAN 29-bit id prefix; the interval field is unidentified. | [CAPTURED 1004] |
| `0xFFDA` | host multi-frame write | h→d | **1004**: `<count=02>` + two 18-byte records (`<i> 00 10 61 04 00 0b 08 07 c0 <8 CAN bytes>`) + `ff`. The two CAN frames are one ISO-TP message (`10 0c …` + `21 …` = 12 bytes) written to 0x7C0. Proves the host path can push multi-frame writes; the payload's meaning is unknown — treat writes as gated. | [CAPTURED 1004] |
| `0xFA4C` | long-record transfer | d→h | **1004**: `<count=1a>` (26) + long records `<u16 BE len> 00 07 e8 10 <innerlen> <innerlen bytes>`; carries the `e0 a8 01` answer (1459 B total). See §6.3. | [CAPTURED 1004] resolved |

Name collision note: `0xFFE4`/`FFD7`/`FFCA`/`FFBD`/`FF6F`/`FF55` (and `0xFFF1`) are one *record list*
format at different counts, not different services: `<count u8>` then `count` × 13-byte records (§6.2).
The count is also encoded in the command's low byte, **`low = (0xF1 − 13·(n−1)) mod 256`** — n=1 lands
on `0xFFF1`, which is why the single-record reply command fits the same ladder (verified n=1,2,3,4,5,11,13;
the arithmetic also recovers n from any `FFxx` whose `(0xF1 − low)` divides by 13, but trusting the
count byte was correct in every 1004 block).

---

## 4. Open sequence (captured order, pacing measured)

The sequence is not decoration: **until the object poll sweep completes, the dongle answers every
frame with the bare `00 00 02 00 00` ack and returns nothing for VIN/DTC/PID reads** — a state that
looks like a healthy link. The sweep is what arms the relay.

```
t≈0.0 s   H→D  FFF8 01 00 04 67 02 01 00          (retry up to 3× only while the dongle is silent)
          D→H  FFFA 00 00 02 00 00                 ~190–515 ms after the first SETUP
t≈2.5 s   H→D  FFF9 01 00 03 60 06 00              STATUS ×3, ~2 s apart
t≈4.5 s   H→D  FFF9 …
t≈6.6 s   H→D  FFF9 …
t≈8.6 s   H→D  FFFA 01 00 02 60 <obj>              object poll sweep, 8 polls, ~40 ms apart:
             cb, cb, d0, c1, c0, c9, cb, c0
t≈9.9 s   H→D  FFF8 SETUP · FFF9 STATUS · FFF8 01 00 04 60 05 07 01
             FFF7 CONFIG · FFF4 TIMING · FFF2 TIMEOUT · FFF5 DESCR   (config burst, ~50 ms apart)
t≈10.0 s  first RECORD/BLOCK data request
```

What each poll returns in the capture:

| Poll payload | Dongle answer | Meaning |
|---|---|---|
| `60 cb` | bare `00 00 02 00 00`, later `FFFB 01 00 01 00` | heartbeat ack |
| `60 d0` | `FFF8 01 00 04 06 10 05 10` | dongle's setup descriptor |
| `60 c1` | `FFEC 01 00 10 f5 ab c9 e8 8c 81 df 5a d6 db a9 45 27 b8 4c 00` | 16-byte identity blob (§6.4) |
| `60 c0` | `FFF8 01 00 04 06 01 07 80` | second descriptor |
| `60 c9` | bare ack | — |
| `60 fd`, `60 25` | echoes active timing config / `00 00 02 00 01` | seen after data flow starts |

Health rules that matter for any client:

* A healthy dongle answers `FFF8` in ~190 ms, not seconds — slow means the SPP channel is held by
  another app (the official one), or the dongle is still booting (~6–10 s after power-up).
* Latch "the dongle announced itself" on the first frame that is **not** `FFFA 00 00 02 00 00` or
  `FFFB 01 00 01 00`. The `FFFB` heartbeat flows even when the relay was never armed.
* Pacing is part of the sequence: the official app spaces STATUS probes ~2 s, polls ~40 ms, config
  burst ~25–50 ms. Answering too fast is fine, but skipping the sweep is not.

---

## 5. Host request payloads

### 5.1 RECORD (`0xFFEC`, exactly 19 data bytes)

```
01 00 10 61 04 00 | 0b | 08 | 07 <obj> | <8 CAN data bytes: <PCI> <request bytes>, zero-padded> | <trailer>
```

* `0b` = u8 record length (11), `08` = DLC, `07 <obj>` = the 11-bit CAN request id 0x7<obj> (§2.1).
* The first CAN data byte is the **ISO-TP PCI = the request's byte count for the 1–7 byte single frames
  seen in every capture** (`02 21 01` sends payload `21 01`; `03 22 d1 00` sends `22 d1 00`;
  `04 30 01 00 8a` sends `30 01 00 8a`). Requests are always one single frame — nothing larger than
  7 bytes was ever addressed on this layer.
* The 8 CAN data bytes are zero-padded; the request length comes from the PCI, never from the field.
* `trailer` is one byte **outside** the CAN frame; captured values `0x14`, `0x01`, `0xff`, `0xfc`,
  `0xfa` (§7.3 lists where each was seen — treat it as a per-request-type constant copied from
  captures, not as free data).

Byte-exact payloads a client must be able to build:

| Request | 19-byte data (after the `FFEC` header) | Evidence |
|---|---|---|
| mode 01 PID 00 (supported bitmap) | `01 00 10 61 04 00 0b 08 07 df 02 01 00 00 00 00 00 00 14` | [CAPTURED 0915] |
| mode 01 PID 0C (1004 trailer `01`) | `01 00 10 61 04 00 0b 08 07 df 02 01 0c 00 00 00 00 00 01` | [CAPTURED 1004] |
| mode 09 PID 02 (VIN, functional) | `01 00 10 61 04 00 0b 08 07 df 02 09 02 00 00 00 00 00 14` | [CAPTURED 0915] |
| mode 07 (pending DTCs) | `01 00 10 61 04 00 0b 08 07 df 01 07 00 00 00 00 00 00 ff` | [CAPTURED 0915] |
| mode 0A (permanent DTCs) | `01 00 10 61 04 00 0b 08 07 df 01 0a 00 00 00 00 00 00 ff` | [CAPTURED 0915] |
| mode 02 freeze frame PID 0C, snapshot 0 | `01 00 10 61 04 00 0b 08 07 df 03 02 0c 00 00 00 00 00 ff` | [CAPTURED 0915] |
| UDS 22 DID `D100` | `01 00 10 61 04 00 0b 08 07 e0 03 22 d1 00 00 00 00 00 ff` | [CAPTURED 0915] |
| UDS 22 on a Ford module obj `20` | `01 00 10 61 04 00 0b 08 07 20 03 22 <hi> <lo> 00 00 00 00 ff` | [CAPTURED 0925] |
| UDS 19 02 8F (read DTC by status mask) | `01 00 10 61 04 00 0b 08 07 e0 03 19 02 8f 00 00 00 00 ff` | [CAPTURED 0915] |
| UDS 3E 00 (tester present, PCI 02) | `01 00 10 61 04 00 0b 08 07 e0 02 3e 00 00 00 00 00 00 ff` | [CAPTURED 0915] |
| UDS 14 FF FF FF (clear, PCI 04) | `01 00 10 61 04 00 0b 08 07 e0 04 14 ff ff ff 00 00 00 ff` | [CAPTURED 0915] |
| mode 21 local ID `01` (Toyota) | `01 00 10 61 04 00 0b 08 07 e0 02 21 01 00 00 00 00 00 ff` | [CAPTURED 1004] |
| UDS 30 01 00 8A (1004 control op, PCI 04) | `01 00 10 61 04 00 0b 08 07 e0 04 30 01 00 8a 00 00 00 ff` | [CAPTURED 1004] |
| mode 06 TID read (`06 35`, PCI 02) | `01 00 10 61 04 00 0b 08 07 df 02 06 35 00 00 00 00 00 01` | [CAPTURED 1004] |
| mode 06 TID bitmap (`06 20`) | `01 00 10 61 04 00 0b 08 07 df 02 06 20 00 00 00 00 00 01` | [CAPTURED 1004] |
| Toyota `a8 01` list request (`fa` trailer) | `01 00 10 61 04 00 0b 08 07 e0 02 a8 01 00 00 00 00 00 fa` | [CAPTURED 1004] |
| Toyota `a8 02` table request (`fc` trailer) | `01 00 10 61 04 00 0b 08 07 e0 02 a8 02 00 00 00 00 00 fc` | [CAPTURED 1004] |
| mode-21 page probe (`00/20/40/60/80/a0/c0`) | `01 00 10 61 04 00 0b 08 07 e0 02 21 00 00 00 00 00 00 ff` | [CAPTURED 1004] |
| mode 09 VIN on the **physical** channel | `01 00 10 61 04 00 0b 08 07 e0 02 09 02 00 00 00 00 00 fc` | [CAPTURED 1004] |
| actuation release (`30 00`, PCI 01) | `01 00 10 61 04 00 0b 08 07 e0 01 30 00 00 00 00 00 00 ff` | [CAPTURED 1004] |
| the `50 c8` probe (no answer) | `01 00 10 61 04 00 0b 08 07 50 c8 01 3e 00 00 00 00 00 ff` | [CAPTURED 1004] |
| mode 01 PID on the UDS channel (wrong channel — don't) | `… e0 03 01 <pid> 00 00 00 00 00 ff` | [BUILT] counter-example: answers nothing 28× |

Whole-frame examples (seq + checksum included) are in §16.2 for regression-pinning your encoder.

### 5.2 BLOCK (`0xFFE8`, exactly 23 data bytes) — send a CAN frame on a stream

```
01 00 14 60 09 03 01 10 | 0b 08 07 | <obj> | 30 00 00 00 00 00 00 00 | <trailer> | 00 00
```

The 8 CAN data bytes are an **ISO-TP flow-control frame** (`30` = FC continue, block size `00`, STmin
`00`) — the receiver's acknowledgement that tells the ECU to send the rest of a multi-frame answer.
The dongle repeats that frame, which is what keeps a long answer alive. That is why this command exists
and why the 1004 streams send it for `e0` (the `21 01` live block) and `c0`. Trailer `ff` (one 0925
variant `fc`); the two `00 00` bytes follow the trailer and are not part of the CAN frame.

### 5.3 Timer record (`0xFFEA`, 1004)

```
01 00 12 61 04 00 | 0d | 88 18 da <00|5a> | <obj> | <8 CAN bytes> | ff
```

4× in 1004, all for a `3e 00` tester-present on `e0`. It is the plain RECORD with the CAN id wrapped as
`88 18 da <tt>` — `18 da` is the UDS-on-CAN 29-bit id prefix, `88` a flags byte, `<tt>` an interval
(`5a` once, `00` otherwise). Effect: the dongle keeps re-sending the record itself, so the app stops
polling 3E. The plain `FFEC` shape works too; use this only if you want the dongle's timer.
[CAPTURED 1004]

### 5.4 Host multi-frame write (`0xFFDA`, 1004)

```
02 | 00 00 10 61 04 00 0b 08 07 c0 10 0c 30 0b 00 00 00 00
   | 01 00 10 61 04 00 0b 08 07 c0 21 40 00 00 00 00 00 00 | ff
```

`<count=02>` then two 18-byte records (`<index> 00 10 61 04 00 0b 08 07 <obj> <8 CAN bytes>`), trailer
`ff`. The two CAN frames are one ISO-TP message — a first frame `10 0c` (12-byte payload) plus a
continuation `21 40 …` — i.e. the host wrote `30 0b 00 00 00 00 40 00 00 00 00 00` to the 0x7C0 module
twice (167.0 s and 170.6 s). This is the host-side mirror of a block reply and proves the write path
exists; the payload's meaning is unknown. Treat every write as gated. [CAPTURED 1004]

---

## 6. Dongle reply payloads

### 6.1 NOTIFY (`0xFFF1`) — one CAN record

```
data:  01 | 00 0b | 08 | 07 <obj> | <PCI> <answer bytes…> <00 padding>
       ^    ^        ^    ^         ^
       |    |        |    |         +- PCI = answer length, 1..7 (ISO-TP single frame)
       |    |        |    +- responding CAN id low byte (e8 = 0x7E8 engine, c8 = 0x7C8)
       |    |        +- DLC = 8
       |    +- record length, u16 BE = 0x000B (DLC + id + 8 CAN bytes)
       +- record count = 1
```

* `e8` = 0x7E8, the ECM — it answers both its own physical reads and every functional `df` (0x7DF) OBD
  read; `c8` = 0x7C8, the module 1004 polled as `c0`.
* The byte after it is the **ISO-TP single-frame PCI = the answer's byte count, for values 1–7**
  (`41 0c 0a 82` declares 4; `47 02 04 42 06 05` declares 6; `61 e3 00 00` declares 4; `7e` declares 1).
  Everything after those bytes is zero padding and **must not be decoded**. (Older text called this
  "declared length including the service byte" — same arithmetic, now with the right name.)
* A record that declares more bytes than the frame actually carries is a **fragment**. Never decode a
  fragment as a value (a truncated `47 02 04 42 06` still reads as a plausible `P0204`); treat it as
  "resend". Fragments happen on the VIN/block stream, not normally on live data.
* A reply is only valid **after** the request that asked for it (reads are pipelined; the next data
  frame is often the previous request's answer).

Byte-exact 1004 samples:

```
01 00 0b 08 07 e8 04 41 0c 0a 82 00 00 00      ->  41 0c 0a 82   = mode 01 PID 0C, 0x0A82 = 2690 rpm
01 00 0b 08 07 e8 04 61 e3 00 00 00 00 00      ->  61 e3 00 00   = mode 21 LID E3 answer
01 00 0b 08 07 e8 03 7f 21 12 00 00 00 00      ->  7f 21 12      = mode 21 refused: subFunctionNotSupported
```

### 6.2 Block replies — `FFE4`/`FFD7`/`FFCA`/`FFBD`/`FF6F`/`FF55` (record lists)

Long answers (mode-21 data, long DIDs, DTC lists, VIN, the `a8` tables) do not fit one CAN frame, so
the dongle batches the ECU's ISO-TP frames into one block frame:

```
data:  <count u8> | 00 0b 08 07 <obj> <PCI> <7 CAN data bytes> | 00 0b 08 07 <obj> <PCI> <7 …> | …
                    \______ one record: u16 BE length 0x000B + DLC + CAN id + 8 data bytes ______/
```

* The **first byte is the record count** (`02`/`03`/`04`/`05`/`0b`/`0d` in the six observed block
  commands; also encoded in the command's low byte, §3). [CAPTURED 1004 verifies all six]
* The command word is **per count** (`FFE4`=2, `FFD7`=3, `FFCA`=4, `FFBD`=5, `FF6F`=11, `FF55`=13).
  Do not hardcode one "block" command — read the count byte, or recover `n` from the low byte
  (`n = ((0xF1 − low) mod 256) / 13 + 1` when it divides evenly).
* The byte after `<obj>` is the **ISO-TP PCI**: `10` = first frame (`window[0]` = total answer length
  in answer bytes — the 1004 VIN declares `14` and delivers exactly 20), then `21`, `22`, … `2F`
  consecutive frames.
* One block can carry **several complete answers**: the 1004 `FF6F` holds a 53-byte and a 14-byte
  message (`10 35 …` then `10 0e …`), `FF55` holds a 62-byte one plus more. Start a new PDU at every
  PCI `10`; never assume one answer per block.
* The declared length in the first frame is a **hint**: it can over-declare vs. what the dongle packed
  (old capture: declared `0x1b`, delivered 17 bytes). Reassemble by taking `min(7, remaining)` from
  each continuation window and emit at the declared length or at the block's last window.
* The last window of a block can be shorter than 7 data bytes (the relay trims it); parse by record
  boundaries, never by fixed offset arithmetic across records.
* `<obj>` is the responding CAN id low byte (`e8` = 0x7E8 engine; `c8` = 0x7C8).

Byte-exact 1004 samples (count word + all records):

```
FFE4 (2 records) — answer to 21 E1 (10 bytes: 61 e1 01 01 0c 46 20 00 02 0a)
02 00 0b 08 07 e8 10 0a 61 e1 01 01 0c 46 00 0b 08 07 e8 21 20 00 02 0a 00 00 00

FFD7 (3 records) — the xB VIN (20 bytes: 49 02 01 "JTL" "ZE4FE1E" "J049314")
03 00 0b 08 07 e8 10 14 49 02 01 4a 54 4c 00 0b 08 07 e8 21 5a 45 34 46 45 31 45 00 0b 08 07 e8 22 4a 30 34 39 33 31 34

FFCA (4 records) — answer to 21 C1 (21 bytes, ASCII "AZE151 2AZFE" + 04 02 41 71 01 00)
04 00 0b 08 07 e8 10 15 61 c1 41 5a 45 31 00 0b 08 07 e8 21 35 31 20 32 41 5a 46 00 0b 08 07 e8 22 45 20 04 02 41 71 01 00 0b 08 07 e8 23 00 00 00 00 00 00 00

FFBD (5 records) — answer to 21 01 (28 bytes: 61 01 + 26 bytes of live data)
05 00 0b 08 07 e8 10 1c 61 01 00 00 00 00 00 0b 08 07 e8 21 25 47 65 85 00 00 00 00 0b 08 07 e8 22 00 00 00 30 82 29 52 00 0b 08 07 e8 23 28 20 01 38 02 33 2f 00 0b 08 07 e8 24 c2 00 00 00 00 00 00
```

Older-capture VIN block (0921, dongle object read — note `10 11 63 …`, **not** a UDS answer):

```
0b 08 07 e8 10 11 63 32 46 4d 48 4b 00   -> "2FMHK" from 2FMHK6DT0BBD17214
0b 08 07 e8 21 36 44 54 30 42 42 44 00   -> "6DT0BBD"
0b 08 07 e8 22 31 37 32 31 00 00 00      -> "1721"
```

A `62 <hi> <lo> …` long DID answer and a `63 …` mode-03 list both arrive this way in 0925; blind
reassembly of a VIN-style object block would fabricate a bogus mode-03 reply out of VIN bytes, so only
reassemble when the first window's PCI is `10` and the payload begins with a known service/answer byte.

### 6.3 Long-record blocks (`FF4F`, `0xFA4C`) — answers a short record cannot carry

The dongle's second record form drops the fixed 8-byte CAN frame and packs a whole answer fragment into
one record:

```
data:  <count u8> | 00 30 00 07 e8 10 2b <43 answer bytes> | 00 35 00 07 e8 10 30 <48 bytes> | …
                    \_ u16 BE len=48 _/  \_ id _/ \_  \_ inner len 43 _/
```

* Record = `00 <u16 BE len> 00 07 <obj> 10 <innerlen> <innerlen answer bytes>` with `len = 5 + innerlen`;
  walk it as `pos += 7 + payload[pos+6]` starting at index 1, bounded by the count byte.
* The 1004 `e0 a8 01` answer arrived as **26 long records in one `FA4C` frame, 1459 bytes total**; the
  same request on `c0` came back as 3 records / 176 bytes in an `FF4F` frame.
* Every long record re-declares its own `10 <len>`: they are *not* ISO-TP continuations of one message,
  but the dongle's own re-chunking into record-sized pieces.
* A client that ignores this family sees a 1459-byte frame whose records don't match the `0b 08 07`
  shape. Parse both families (§6.2 and this one) or skip the command explicitly.

### 6.4 Dongle identity blob (dongle → host, `0xFFEC`)

```
01 00 10 f5 ab c9 e8 8c 81 df 5a d6 db a9 45 27 b8 4c 00
```

16 bytes, static across captures and byte-identical on 0915/0921/1004 (so **not a nonce**). Arrives
after polling object `c1`. No reply is required. Safe to log and ignore; compare across dongles to tell
firmware/hardware apart. Field meanings unknown. [CAPTURED]

### 6.5 Other dongle frames

* `FFF8 01 00 04 06 10 05 10` / `01 00 04 06 01 07 80` — descriptor announcements (§4).
* `FFFA 00 00 02 00 00` — poll ack. `FFFB 01 00 01 00` / `01 00 01 01` — bare ack. `FFFC 01 ff ff` — end.
* Poll ACKs are sometimes retransmitted with the same seq (128 duplicates in 0925) — harmless.

---

## 7. Channels: CAN id / PCI / trailer

With §2.1's reading: `obj` is the **low byte of the 11-bit CAN request id** (so it identifies the ECU,
0x7xx), the byte older text called `slot` is the **ISO-TP PCI** (= the request payload length for the
1–7 byte single frames every capture shows), and `trailer` is a dongle directive outside the CAN frame.
The table keeps the familiar `obj`/number columns; read the first two columns as CAN id / PCI.
**A wrong channel does not fail loudly** — the dongle answers nothing, or a bare ACK, which reads as
"this car does not support it".

### 7.1 The channel table

| obj (CAN id) | PCI | service family | trailer | Evidence |
|---|---|---|---|---|
| `df` (0x7DF) | `02` | modes 01 / 06 / 09 (functional OBD) | `14` (0915/0921) · `01` (1004) | [CAPTURED] |
| `df` | `01` | modes 07 / 0A DTC lists; the app's own `82` query | `ff` (`14` once for `82`) | [CAPTURED] |
| `df` | `03` | mode 02 freeze frames, one call per PID | `ff` | [CAPTURED] |
| `e0` (0x7E0) | `03` | UDS 22 / 19 / 18 on the engine ECU | `ff` | [CAPTURED] |
| `e0` | `02` | UDS 3E 00; **1004: mode 21, mode 09 VIN, `a8 01/02`** | `ff` (`fc` on first/re-arm reads; `fa` on `a8 01`) | [CAPTURED 1004] |
| `e0` | `04` | UDS 14 FF FF FF clear; **1004: `30 01 00 <arg>` control op** | `ff` | [CAPTURED] |
| `e0` | `01` | **1004: `30 00` release** (PCI 01 = single byte `30`) | `ff` | [CAPTURED 1004] |
| `20`,`26`,`44`,`60`,`61`,`a7` | `03` | UDS 22/19 on other physical ECUs (Ford 0925); `a7` also 31/2F/10/14 | — | [CAPTURED 0925] |
| `c0` (0x7C0) | `02`/`01` | 1004's second module: `21 <page>`, `a8 01/02`, `3e 00`, `30 00`; answers on 0x7C8 | `ff`/`fa`/`fc` | [CAPTURED 1004] |
| `50` (0x750) | — | 1004's `50 c8 01 3e` probe: the first data byte `c8` is **not** an ISO-TP PCI and nothing answered | `ff` | [UNCLEAR] [CAPTURED 1004] |

Implemented constants (`A30Session`): `OBD_OBJ=0xDF, OBD_SLOT_STREAM=0x02, OBD_SLOT_LIST=0x01,
OBD_SLOT_FREEZE=0x03, UDS_OBJ=0xE0, UDS_SLOT=0x03, UDS_SLOT_TESTER=0x02, UDS_SLOT_CLEAR=0x04`,
trailers: stream `0x14` (official) / `0x01` (1004), everything else `0xFF` (plus `fc`/`fa` first-read
variants in 1004).

### 7.2 Per-module objects (Ford, 0925)

| obj | module | Note |
|---|---|---|
| `20` | PCM | live-data channel; DID 6308 polled 265× over ~40 s; no mode-01 used |
| `26` | TCM | carries the 7xxx DID block |
| `44` | ABS | shares A5xx / D1xx |
| `60`,`61` | body-ish | also answered UDS 19 |
| `a7` | richest | DID D100 polled 430×, took session control, routine 31, and clear |
| `df` | functional | modes 01/02/07/09/0A and VIN |

The official app addresses **six modules with six objects** in one session; a DID from one module sent
to another answers nothing. The android app models this as a per-brand module table (`assets/brand_modules.json`).
With §2.1's reading these objects are CAN id low bytes too (0x720/0x726/0x744/0x760/0x761/0x7A7),
consistent with the community packs' `Header` column (7E0/7E1/726…) — worth confirming on a capture.

### 7.3 Trailer variants seen (do not invent values)

| Trailer | Where |
|---|---|
| `0x14` | official app modes 01/09 on `df 02` (0915/0921) |
| `0x01` | 1004's mode-01 stream on `df 02` (and one `01 00` request) |
| `0xff` | everything else in both apps |
| `0xfc` | 1004 first-time/re-arm reads (`e0 02 21 28`, first `21 c1`, `e0 02 09 02`, `c0 02 a8 02`) |
| `0xfa` | 1004 `e0 02 a8 01`, `c0 02 a8 01` |

The difference between `14` and `01` for the same request is unexplained; both were answered. Copy the
variant from the capture for the service you use, and tolerate either when parsing.

### 7.4 Routing rules a client must not break

* J1979 services (01/02/03/04/06/07/09/0A) are answered on the functional `df` (0x7DF) — except mode 09,
  which 1004 read on the physical `e0` too. Sending `01 <pid>` on `e0` produced 28 empty answers in our
  first build — the expected result of the wrong channel, not a vehicle limitation.
* UDS 22/19/18 ride a **physical** object (`e0` for the engine, `20` etc. per module); 14 uses PCI `04`.
* 1004's mode-21 / `a8` / VIN reads ride `e0` **PCI 02** — a second, real variant of the same physical
  channel. That app never sent mode 21 on PCI 03; the a30probe Probe 21 now uses the captured PCI 02
  shape too (see §9.5).
* Mode-01 bitmap first (`01 00`); a PID absent from the bitmap will never answer (1004 proves the
  negative: `01 A6` and `01 D3`, absent from the bitmap, stayed silent).
* The responding CAN id is request id + 8 for physical addresses (0x7E0→0x7E8, 0x7C0→0x7C8); the
  functional 0x7DF is answered by the ECM on 0x7E8. Don't key reply handling on the request object alone.

---

## 8. Services observed on the vehicle link

| Service | Use | Answer | Evidence |
|---|---|---|---|
| `01` | mode-01 PID read (functional) | `41` + pid + data | [CAPTURED] |
| `02` | mode-02 freeze frame; reply repeats the snapshot number before the value | `42 <pid> <snapshot> <data>` | [CAPTURED] |
| `03` | stored DTC list (legacy) | `43` + count + 2-byte pairs | [CAPTURED Scion session] |
| `04` | legacy clear | `44` | [UNCLEAR — never captured; an xB-style ECU may accept it] |
| `07` | pending DTC list | `47` + count + 2-byte pairs (`47 02 04 42 06 05` = P0442, P0605) | [CAPTURED] |
| `06` | on-board monitor tests: `06 <00|20|40|60|80|a0>` bitmaps, then one `06 <tid>` per supported TID | `46 <tid>` + N × 9-byte results (§8.1) | [CAPTURED 1004] |
| `09` | mode-09 info; PID 02 = VIN (`49 02 01` + 17 ASCII). 0915 read it on `df 02`, **1004 on `e0 02`** | `49` | [CAPTURED] |
| `0A` | permanent DTC list | `4A` + count + 2-byte pairs | [CAPTURED] |
| `10` | UDS diagnostic session control per module (`10 81`/`10 87`) | `50` | [CAPTURED 0925] |
| `14` | UDS clear DTC (group `FF FF FF`), PCI 04 | `54` in ~137 ms (Ford) · `7F 14 11` (Scion xB) | [CAPTURED] |
| `18 00 FF 00` | sent by the official app after module connect (7× in 0925) | `7F 18` serviceNotSupported | [UNCLEAR] |
| `19` | UDS readDTCInformation (`19 02 8F`, `19 04 06 <dtc> <record>`) | `59` or `7F 19 12` | [CAPTURED] |
| `21` | **mode-21 local-identifier read** (Toyota/Lexus/Scion dialect) | `61 <lid> <data>` / `7F 21 <nrc>` | [CAPTURED 1004] — first ever |
| `22` | ReadDataByIdentifier — the live-data path on UDS cars | `62 <did hi> <did lo> <data>` | [CAPTURED] |
| `23` | ReadMemoryByAddress — the official app reads ECU memory with it | data | [CAPTURED 0921, shape only] |
| `2E` | writeDataByIdentifier | — | never observed; treat as destructive, do not send |
| `2F` | routine control (`2F 63 08 00`) | `6F` | [CAPTURED shape only] |
| `30` | InputOutputControlByLocalIdentifier — 1004 swept `30 01 00 <v>` (`8a 94 9e a8 b2`, back down to `62`) on `e0 04`, then `30 00` on `e0 01` | `70 01` (2 B) per set, then `70` (1 B) | [CAPTURED 1004] |
| `31` | routine control (`31 02 00`) | `71 02`, `73 02` | [CAPTURED 0925] |
| `3E` | tester present (`3E 00`) | **1004: `7e` (1 B) on `e0 02`, accepted** — same ECU that refused it in the 0921 session (`7F 3E 12`, §14.7) | [CAPTURED] |
| `82` | the official app's own DTC query (one byte) | end-of-data; meaning unknown | [CAPTURED] |
| `A8` | Toyota list service. `A8 01` → a long identifier list (`E8 01` + entries shaped `<id> <len> <len bytes>`; 26 long records / 1459 B on `e0`, 4 / 176 B on `c0`); `A8 02` → a short table (`E8 02`, 53 B on `e0`, 62 B on `c0`) | `E8 01`/`E8 02` | [CAPTURED 1004] |

Negative responses: `7F <service> <nrc>`, two bytes after the `7F` (the refused service then the NRC).
NRCs named by the app: `10` generalReject, `11` serviceNotSupported, `12` subFunctionNotSupported,
`13` incorrectMessageLength, `21` busyRepeatRequest, `22` conditionsNotCorrect, `24` requestSequenceError,
`31` requestOutOfRange, `33` securityAccessDenied, `35` invalidKey, `72` generalProgrammingFailure,
`78` responsePending, `7E`/`7F` session-variant refusals. An empty answer and a `7F` refusal are
**different facts**: `7F` = the ECU heard and refused; silence = wrong channel/session (or the request
arrived after your window).

### 8.1 Mode 06 — captured shape (1004, Scion xB)

Requests ride `df 02` with trailer `01`; each TID bitmap answer is 4 bytes covering 0x20 TIDs:

| Request | Answer | TIDs the bitmap advertises |
|---|---|---|
| `06 00` | `46 00 c0 00 00 01` | 01, 02, 20 |
| `06 20` | `46 20 80 00 08 09` | 21, 35, 3D, 40 |
| `06 40` | `46 40 40 00 00 01` | 42, 60 |
| `06 60` | `46 60 08 00 00 00` | 65 |
| `06 80` | `46 80 00 00 00 00` | none |
| `06 a0` | `46 a0 f8 00 00 00` | A1–A5 |

The app then read the supported TIDs. Result format (J1979-2-like): `46 <tid>` followed by N × 9 bytes
`<tid> <MID> <UAS> <value u16 BE> <min u16 BE> <max u16 BE>`:

```
06 35 -> 46 35 81 35 00 00 00 00 03 b5
         one result: MID 81, UAS 35, value 0, min 0, max 949
06 a2 -> 46 a2 0b 24 00 00 00 00 ff ff a2 0c 24 00 01 00 00 ff ff
         two results: MID 0B value 0 / MID 0C value 1, min 0, max 65535
06 a5 -> 46 a5 0b 24 00 00 00 00 ff ff a5 0c 24 00 00 00 00 ff ff
         same shape, both values 0
```

MID/UAS are the standard's field names; treat the numeric fields as literal until scaling is verified.

### 8.2 Service 30 — actuation sweep (1004)

Thirteen `30 01 00 <v>` writes on `e0` PCI 04 ramped the argument up and back down — `8a 94 9e a8 b2`,
return `a8 9e 94 8a 80 76 6c 62` — over ~8 s, each answered `70 01` (2 bytes); then one `30 00` on
`e0` PCI 01 answered `70` (1 byte). That is an input-output control ("active test") on local identifier
01: read the pair as *set* then *release*. Which actuator it drove is not visible in the bytes.

---

## 9. Mode 21 — Toyota local-identifier data ([CAPTURED 1004] for the first time)

Mode 21 (`ReadDataByLocalIdentifier`) is the native Toyota/Scion/Lexus live-data dialect. Before
2026-10-04 no capture contained a single mode-21 exchange; the 1004 capture is a full session of it
against the Scion xB, and it settles the request/reply shapes.

### 9.1 Request

```
e0 02 21 <lid> 00 00 00 00 00 ff     (inside the 19-byte FFEC record: CAN 0x7E0, PCI 02, trailer ff)
```

* `<lid>` is one byte (0x00–0xFF) **within one module's id space** — do not interleave modules.
* 1004 sent `e0 02` (engine CAN 0x7E0, PCI 02) for every mode-21 read; a `fc` trailer appears on some
  first-time/re-arm reads. The same channel carried `09 02` (VIN), `a8 01/02`, and `3e 00`; mode 21 was
  never sent on PCI 03 by that app.
* The request payload is exactly `21 <lid>` zero-padded to 7 bytes, trailer `ff` (`fc` variant).

### 9.2 Reply

Positive: `61 <lid> <data…>`, negative: `7F 21 <nrc>`. The answer's **transport depends only on its
length** — this is the part a new client must implement fully:

| Answer size | Frame | Records | Example (1004) |
|---|---|---|---|
| ≤ 7 bytes | `FFF1`, one CAN record (PCI = answer length) | 1 | `61 e3 00 00` |
| 8–13 bytes | `FFE4` (count `02`) | 2 | `61 e1 01 01 0c 46 20 00 02 0a` (10 B) |
| 14–20 bytes | `FFD7` (count `03`) | 3 | VIN `49 02 01 …` (20 B) |
| 21–27 bytes | `FFCA` (count `04`) | 4 | `61 c1 41 5a 45 31 35 31 20 32 41 5a 46 45 20 04 02 41 71 01 00` (21 B) |
| 28–34 bytes | `FFBD` (count `05`) | 5 | `61 01` + 26 B (28 B, 424×) |
| 35+ bytes | not seen for mode 21; 1004's `FF6F`/`FF55` blocks carried the `a8 02` tables (§8) | — | — |

(For len > 7, records = 1 + ceil((len−6)/7): the first record carries 6 data bytes, each continuation
up to 7. The size ranges above are that arithmetic, not a separate rule; ≤ 7 bytes travel in a single
`FFF1` record.)

### 9.3 Observed local IDs and their exact answers

| LID | Polls | Answer | Transport |
|---|---|---|---|
| `01` | 424, streamed | 28 B: `61 01` + 26 data bytes (live block, changes every sample) | FFBD |
| `e3` | 112 | 4 B: `61 e3 00 00` | FFF1 |
| `c1` | 57 | 21 B: `61 c1 41 5a 45 31 35 31 20 32 41 5a 46 45 20 04 02 41 71 01 00` — ASCII **"AZE151 2AZFE"** (the xB chassis code and 2AZ-FE engine) + calibration bytes | FFCA |
| `e1` | 56 | 10 B: `61 e1 01 01 0c 46 20 00 02 0a`; the last byte rises steadily through the run (timer/counter) | FFE4 |
| `e0` | 4 | `61 e0 e0 00 00 00` on `e0`; `61 e0 c0 00 00 00` on `c0` (the answer echoes the CAN id) | FFF1 |
| `e2` | 2 | `61 e2 f7 f7 d9 f3 70` on `e0`; `61 e2 1e d9 11` on `c0` | FFF1 |
| `00`,`20`,`40`,`60`,`80`,`a0`,`c0` | 1 each, then the same sweep on `c0` | page answers `61 00 bc 00 00 01` / `61 20 ac 00 22 d1` / `61 40 9c 00 d0 01` / `61 60 00 00 00 01` / `61 80 c8 00 00 01` / `61 a0 d3 f9 c0 01` / `61 c0 fe 80 80 01` — they read like "which LIDs exist in this 0x20 page" and are what the app used to pick `c1/e1/e3` | FFF1 |
| `28` | 1 | `7F 21 12` — subFunctionNotSupported | FFF1 |

The app did not guess the LID list: before streaming it read `a8 01` on `e0` (26 long records, §6.3)
and this page sweep, then polled `01`, `c1`, `e1`, `e3`.

The `21 01` live block changes every sample; one exact reassembly from the capture:

```
FFBD: 05 00 0b 08 07 e8 10 1c 61 01 00 00 00 00 00 · 0b 08 07 e8 21 25 47 65 85 00 00 00 00 ·
      0b 08 07 e8 22 00 00 00 30 82 29 52 00 · 0b 08 07 e8 23 28 20 01 38 02 33 2f 00 ·
      0b 08 07 e8 24 c2 00 00 00 00 00 00
reassembled:  61 01 00 00 00 00 | 25 47 65 85 00 00 00 | 00 00 00 30 82 29 52 | 28 20 01 38 02 33 2f | c2
              (first frame = 6 data bytes, each continuation = 7; the window's extra byte is relay pad)
```

### 9.4 Cadence and session shape

* **Identify (23–38 s)** — `e0 02`: `3e 00`, `21 c1` (ECU id string), `21 28` → `7f 21 12`, `09 02`
  (VIN), `21 e2`, `21 e0`, then the page sweep `21 00/20/40/60/80/a0/c0` and `a8 01` (the big list).
* **Live stream (39.6–89 s)** — `21 01` (28 B, 4.8 Hz over the run), `21 e3` (4 B, 3.5 Hz), `21 e1`
  (10 B, 1.7 Hz), `21 c1` (21 B, ~1 Hz); 646 mode-21 reads on `e0` plus 10 on `c0`, measured gap
  min 34 ms / mean ≈83 ms.
* **Housekeeping (95–127 s)** — `a8 02` on `e0` (53 B), the `30` actuation sweep (§8.2), then `30 00`.
* **Second module (146–151 s)** — `c0`/0x7C0 repeats the identify shapes (`3e 00`, `21 e2`, `a8 02`,
  `21 e0`, page sweep, `a8 01`); answers come back on `c8`/0x7C8; `30 00` closes it at 175 s. A `FFDA`
  multi-frame write to 0x7C0 sits at 167/171 s (§5.4).
* **Mode-01 scan (193–228 s)** — four bitmaps then every advertised PID, ending in a 23 s `01 0C`
  stream at 4 Hz (§10.1).
* **Mode 06 (233–248 s)** — bitmaps then TIDs `35`, `a2`, `a5` (§8.1).
* Throughout, the session opens with the standard sequence (§4); the dongle relays answers ~30–150 ms
  after each request.

### 9.5 What this means for an existing client

* Parse **every record command**: `FFF1` (n=1), `FFE4`, `FFD7`, `FFCA`, `FFBD`, `FF6F`, `FF55`, plus the
  long-record family (`FF4F`/`FA4C`, §6.3). Otherwise the 10-byte, 21-byte and 28-byte answers are
  invisible and the `a8` lists are unparseable. The a30probe app as of 2026-10-04 parses only `FFCA`;
  the 1004 capture shows that is not enough. **Resolved (a30probe, 2026-10-04)**: the app now
  reassembles all six record-list commands (`A30Session.reassembledBlockReplies`) and walks the
  long-record family (`longRecords`), with the xB's own block bytes pinned in `tools/TranscriptCheck.kt`
  and `a30poc/transcript.py` kept in transcript parity.
* The known-good channel is `e0` **PCI 02** for mode 21 (the VIN, `a8` and `3e` reads rode it too).
  The a30probe Probe 21 builder used PCI 03 — untested against a car; when probing a real Toyota,
  prefer the captured `e0 02` shape, or send both and see which the ECU honours. **Resolved
  (a30probe, 2026-10-04)**: `buildLocalId` sends the captured `e0 02 21 <id>` shape, and the probe
  table carries the xB's six answered ids (raw, no equation) ahead of the packs' engine rows.
* Anything longer than one CAN frame needs the `FFE8` flow-control frame (§5.2), or the ECU stops
  mid-answer.
* A reply shorter than the formula expects must stay raw bytes; never zero-pad a value (§10.2–10.4).

---

## 10. Value decoding and catalogs (what to do with the bytes)

### 10.1 Mode-01 PIDs

Standard SAE J1979: `41 <pid> <A> <B> …`. Bitmaps gate the menu: `01 00` covers PIDs 01–20, and when
its last bit is set the next block is read (`01 20`, `01 40`, `01 60`). The 0915 Ford answered
`41 00 bf be a8 93`; the 1004 Scion xB answered all four blocks:

```
41 00 be 1f a8 13    01,03,04,05,06,07,0C,0D,0E,0F,10,11,13,15,1C,1F,20
41 20 90 05 b0 15    21,24,2E,30,31,33,34,3C,3E,40
41 40 fa dc a0 01    41,42,43,44,45,47,49,4A,4C,4D,4E,51,53,60
41 60 08 00 00 00    65
```

(`60` is itself the "PIDs supported 61–80" PID — that bit is why the app read `01 60` next; `65` is the
only value PID in the 61–80 block.)

The app then polled every advertised PID once in **descending** order (`65 53 51 4E 4D 4C 4A 49 47 45
44 43 42 41 3E 3C 34 33 31 30 2E 24 21 1F 1C 15 13 11 10 0F 0E 0D 0C 07 06 05 04 03 01`), finishing
with a 23 s `01 0C` (RPM) stream at 4 Hz. Two PIDs absent from the bitmaps (`01 A6`, `01 D3`) were
probed first and stayed silent — a useful negative control. Build your own table or reuse the 165-entry
J1979 catalog compiled by `tools/build_pid_catalog.py` into `PidCatalogData.kt`.

### 10.2 Formula dialect (J1979/ODBDash, used for PIDs and DIDs)

A tiny expression language: `A`–`Z`, `AA`, `AB`… are payload bytes in Excel-column order (A=0 … H=7,
AA=26); `signed16(A,B)` / `signed8(A)` are the only functions; `;`-separated `name=expr` parts render
compound values. Division by zero yields "no value". A missing byte (answer shorter than the formula)
is **zero-padded** for live surfaces; a caller that must not show padding asks
`bytesNeeded(formula)` (= highest byte index + 1) first and keeps raw bytes if the answer is shorter.
Reference implementation: `a30probe/.../PidMath.kt`, mirrored by `tools/pidgrammar.py`.

### 10.3 DTCs and freeze frames

* Mode 03/07/0A lists: count byte then 2-byte DTC pairs (`47 02 04 42 06 05` = P0442 + P0605).
* UDS 3-byte DTC (service 19 answers): first byte top two bits = family P/C/B/U, then 2-digit + 1-digit + 1-digit, optional `/FTB` suffix.
* **Mode 02 reply layout**: the snapshot number is echoed between the PID and the value — `42 0C 00 1A 1E`
is **1672 rpm**, not 6; `42 05 00 4E` is **38 °C**, not −40. Drop the echoed snapshot byte when it
matches the requested one. In mode 02, PID `02` is the 2-byte DTC the snapshot belongs to, and PID `00`
is the mode-02 supported bitmap — neither is a normal value.
* Clear verdict: Ford granted `14 FF FF FF` with `54` in 137 ms; the xB refused it (`7F 14 11`) and mode 04
is unverified there. A clear ladder must stop after an explicit negative, and should say which clear
(if any) was accepted.

### 10.4 Branded mode-22 DID catalog

701 DIDs over 11 brands, generated from community OBDDash packs (`DidCatalogData.kt`, built by
`tools/build_did_catalog.py`); mode-21 rows are **not** imported into it (separate dialect). The 0925
Ford session's own DIDs without a sourced formula (6308, 630B, 630F, A431, D100…, 20 of them) ship as
raw-watch entries: stream and log hex until a formula exists. The catalog is a menu, not an authority —
the car's own `7F 22` replies prune it per vehicle. `DID 6308` cycled `00/02/58` (a bitfield/state, not
a scaled number).

### 10.5 The mode-21 probe table (a30probe asset)

`assets/local_ids.json` lists 12 engine-header Toyota rows (`2105` coolant, `210C` RPM, `210D` speed,
`210F` IAT, `2111` TPS, `2104` load, `210E` ignition advance, `2110` MAF, `2114` O2, `2106`/`2107` fuel
trims, `21E4` injection pulse) with their pack equations, for the app's user-invoked Probe 21. The 1004
capture shows the official-style app reading `01/c1/e1/e3/…` instead — the local-id space is large;
the probe table is a starting point, not the whole dialect.

---

## 11. Timing summary

| Event | Timing | Evidence |
|---|---|---|
| SETUP → first answer | ~190 ms (0915) … ~515 ms (1004) | [CAPTURED] |
| Whole open sequence | ≈ 9–10 s incl. the sweep; the official app waits ~6 s after RFCOMM opens before its first frame | [CAPTURED] |
| Poll sweep | 8 polls, ~40 ms apart | [CAPTURED] |
| UDS 22 round trip | ~92–150 ms | [CAPTURED] |
| Clear ack (`54`) | ~137 ms | [CAPTURED] |
| mode-21 answer | ~30–150 ms after the request | [CAPTURED 1004] |
| mode-21 stream gap | min 34 ms, mean ≈83 ms | [CAPTURED 1004] |
| Mode-01 full scan | 199 polls in ~14 s (1004), then `01 0C` at 4 Hz for 23 s | [CAPTURED 1004] |
| Mode-06 TID read | ~200 ms round trip (1004) | [CAPTURED 1004] |
| Actuation writes | 13 `30 01 00 v` writes in ~8 s, then `30 00` (1004) | [CAPTURED 1004] |
| Official live cadence | ≈150 ms per DID (0925: DID 6308 polled 265× / ~40 s) | [CAPTURED 0925] |
| Idle keepalive | dongle poll + `3E 00` every ~4 s | [BUILT] |

---

## 12. Vehicle behavior profiles (data, not code)

A profile only **pre-orders** variations the failover would try anyway; a wrong profile may waste safe
round trips but must never authorize an unverified alternate clear. Shipped profiles
(`assets/brand_profiles.json`, contract in `brand_profiles.contract.md`):

| id | label | keepalive | clear order | VIN first | Evidence |
|---|---|---|---|---|---|
| `generic` | Generic OBD-II | auto | uds-first | 0915 dance | fallback for everything |
| `toyota` | Toyota / Scion / Lexus | **skip** | **mode04-first** | false | 2014 xB to our app: `7F 3E 12` every keepalive, `7F 14 11` on clear, mode 03 granted, mode 04 unverified. (1004 shows a different session got `7e` for `e0 02 3e 00` — §14.7; `skip` is our client's choice, not a vehicle fact.) |
| `ford` | Ford / Lincoln / Mercury | auto | uds-first | false | 0925: `54` granted, VIN via FFD7 + `09 02`, DIDs in FFCA, UDS routed on obj `20` |
| nissan, mazda, renault, chrysler_dodge_jeep, gm/opel, subaru, volkswagen | catalog-derived placeholders | auto | uds-first | VW=true | `[unproven on this dongle]`; behaviorally Generic |

Profile semantics that matter to a client: `keepalive=skip` never sends `3E 00` (the refusal costs a
round trip per pass); `auto` probes once and drops it after the first `7F 3E`; `vinF190First` swaps the
UDS `22 F1 90` read ahead of the three-frame mode-09 dance. `udsObj` sets the default physical object
(Ford `0x20`, others `E0`).

---

## 13. Failure diagnosis (the part that saves hours)

| Symptom | What it actually means |
|---|---|
| Dongle answers **nothing at all** | Your frame is missing/short the XOR octet, or the length field doesn't match. It never errors — it desynchronizes on the next magic and goes permanently silent. Re-check §2 first. |
| Every answer is `00 00 02 00 00` (bare poll ack) | The object poll sweep never ran, or ran before the dongle was ready. The link is up; the relay is not armed. Run the sweep. |
| Only `FFFB 01 00 01 00` arrives | Same trap as above, one step later: the heartbeat ack flows even when the relay is dead. Don't latch "session alive" on it. |
| A read answers nothing while others work | Wrong CAN id / PCI / trailer for that service (§7). The dongle does not report the mistake. |
| `7F <svc> 11`/`12` | The ECU heard and refused; this is a fact about the vehicle, not the tool. |
| A reply looks plausible but is wrong | A truncated record (declared length not fully present) decoded as a value. Drop fragments; resend. |
| Same reply arrives twice / stale value | Reads are pipelined. Accept only replies that arrived after your request (compare arrival counts/timestamps). |
| Long answer invisible | You only parse one block command. Parse all record-window blocks (§6.2). |
| Link dies mid-stream | The dongle can drop RFCOMM (observed ~90 s into one 0922 session); end the loop at the next pass, don't keep writing. |

A `7F 14 11` on a clear is a **vehicle choice** (the xB refuses the UDS clear) — but that does not prove
mode 04 is safe or accepted there. Stop after the explicit negative; report which clear was accepted, if any.

---

## 14. Open questions (what to confirm with a future capture)

**Resolved by the 1004 re-analysis** (kept here so earlier text can't mislead): the block-command
numbering (`low = (0xF1 − 13·(n−1)) mod 256`; `FFF1` is n=1); `FF4F`/`FA4C` are the long-record family
(§6.3); `FFEA`/`FFDA`/`FFE6` are host-side timer/write/setup wrappers (§3, §5.3–5.4); `07 e8 10 2b …`
is the ECM CAN id + ISO-TP first frame, not a mystery blob; `a8 01`/`a8 02` are list/table reads (§8).

Still open:

1. **`21 01` live-block fields** — 28 bytes that update every sample (some bytes step 1–2 per poll);
   map them to physical values with a simultaneous known-value log.
2. **`A8 01`/`A8 02` field semantics** — entries look like `<id> <len> <len bytes>`; 1004 gives sizes
   only (`e0`: 1459 B list / 53 B table; `c0`: 176 B / 62 B).
3. **Mode-21 page bitmaps** — `21 00/20/40/60/80/a0/c0` return bitfield-looking answers; confirm the
   "which LIDs exist" reading by comparing against `21 <lid>` reads of every id.
4. **The `50 c8` channel** — object `50` (CAN 0x750) sent `c8` as its first data byte (not an ISO-TP
   PCI) and got no answer; a different addressing mode for that module?
5. **`30 01 00 <v>` semantics** — which local identifier/actuator, and why `70 01` echoes only `01`.
6. **`FFEA` timer fields** — `88 18 da <00|5a>`: which field is the repeat interval, and is it time or
   poll count?
7. **The 3E contradiction** — 1004 got `7e` for `e0 02 3e 00`; the 0921 xB answered `7f 3e 12` to our
   app's 3E. Which field changed (trailer? session order? PCI?) decides how safe a keepalive is.
8. **`0xFFE6`** — the 25-byte block variant (`… e0 30 00 04 00 00 00 00 00 fc`) vs `FFE8`'s
   `e0 30 00 00 …`: what does the extra `04` configure?
9. **Service `18 00 FF 00`** — sent right after module connect; refused here (`7F 18`). Older DTC spec
   or vendor service?
10. **Trailer semantics** — `14` vs `01` on the same `df 02` mode-01 request, `fc`/`fa` on first reads.
    Is it a stream flag, a timeout, or just app generation? *Test:* flip it on a known-read car.
11. **Write/programming path** — `2E`/`31`/`34-36-37` never captured, but 1004's `FFDA` proves the host
    can write multi-frame messages (12 bytes to 0x7C0, §5.4). The official app ships 26+ service
    functions (oil reset, EPB, SAS, DPF regen, throttle relearn…) on this layer; their shapes are
    uncharted. Treat every write as destructively gated.
12. **Identity blob fields** — 16 bytes, one dongle sample; which byte is firmware/hw/serial?
13. **`END` (`FFFC`)** — 28× at 0925 teardown but also mid-session at 41.8 s/83.6 s. Per module? Per
    channel?
14. **`FFE4`/`FFD7` duplicate-count risk** — when two answers are pending, do records from one block
    interleave with another? (Not seen; guard by pairing request→block.)
15. **Object numbering beyond Toyota** — `obj` = CAN id low byte now explains `e0`/`df`/`c0`/`50`;
    Ford's `20/26/44/60/61/a7` should map to 0x720/0x726/0x744/0x760/0x761/0x7A7 — verify with a capture.

---

## 15. Minimal client recipe (SPP, any language)

1. Bond the A30M-… device in OS/Android settings. It needs the stored link key; no app-level auth.
2. Open `createInsecureRfcommSocketToServiceRecord(00001101-…)` to the bonded MAC.
3. Encoder: `55 AA | seq | len | cmd | data | xor`; seq starts at 1 and increments per frame; on every
   inbound frame, parse by magic `AA 55`, `len`, verify XOR, then decode.
4. Run the open sequence exactly as §4 (SETUP retry-while-silent, 3× STATUS ~2 s apart, the 8-poll
   sweep, the config burst). Latch "armed" only on a non-bare-ack dongle frame.
5. To read:
   * J1979: `FFEC` record `df 02|01|03` + trailer `14/01|ff|ff` (copy the table §7.1).
   * UDS DID: `FFEC` record `<obj> 03 22 <did> … ff` on the module object (engine `e0`; Ford modules
     `20/26/44/60/61/a7`).
   * Mode 21: `FFEC` record `e0 02 21 <lid> … ff`; parse answers from `FFF1` **and every
     record-window block** (§6.2), reassembling ISO-TP (`10` first frame, `21…2F` continuations,
     `min(7, remaining)` per window).
6. Pair replies to requests by arrival order/timestamp; drop truncated records; keep `7F` negatives as
   first-class outcomes with their NRC.
7. Optional housekeeping: poll object `c1` once to log the identity blob; send `3E 00` on `e0 02` every
   ~4 s only if the ECU doesn't refuse it; stop everything on `FFFC 01 ff ff` teardown if you see it.
8. Safety defaults worth copying: never send `2E`, `31` write-routines, `34/36/37`, or mode 04 without a
   capture proving the vehicle accepts it; stop a clear ladder on the first explicit `7F`.

---

## 16. Provenance

### 16.1 Source captures

| File | What it proves |
|---|---|
| `btsnoop_hci_202609151523.cfa` | official app / Ford: full open, VIN `2FMHK6DT0BBD17214`, mode-01 bitmap + live PIDs, freeze frames ×42, mode 07/0A lists, UDS `D100`/`F188`, `54` clear ack, checksum proof |
| `btsnoop_hci_202609210050.cfa` | second day, byte-identical host frames; our early checksum-less sessions (the counter-example); 2014 Scion xB refusals (`7F 3E 12`, `7F 14 11`, mode 03 granted) |
| `btsnoop_hci_202609251529 ford multiple module and some live datas.cfa` | six Ford module objects, per-module DIDs, mode-22 live stream (DID 6308 polled 265×), 10/31/14 shapes, RFCOMM credit/multi-UIH facts, 3672/3672 checksums |
| `btsnoop_hci_20261004130-.cfa` | **Toyota full ECM session** (different app build): VIN `JTLZE4FE1EJ049314` via `e0 02 09 02`; 424 × `21 01` 28-byte live block plus `21 c1/e1/e3`, page sweeps, `a8 01/02`; **mode-01 full scan** (4 bitmaps + every advertised PID + a `01 0C` stream); **mode 06** (`06 35/a2/a5`, 9-byte results); **service 30** actuation sweep; a second module on 0x7C0; new commands `FFE6/FFEA/FFDA/FF4F/FA4C`; **993/993 host and 1979/1979 dongle XORs** |

Tooling that produced/verifies these facts: `tools/capture_db.py` (request catalogue/pairing),
`tools/snoop_tail.py` (session decode; note: its DLCI auto-pick can miss the A30 host stream on mixed
captures — filter handle 0x13/DLCI 12), `tools/capture_checksums.py`, `tools/security_audit.py`,
`btsnoop.py`, and `tools/check_transcript.sh` (compiles the real Kotlin builders against every captured
frame — 731+ checks).

### 16.2 Whole captured frames (seq + checksum) for regression pinning

Official app / 0915–0921, reproduced by this repo's builders byte-for-byte:

```
SETUP  55 aa 00 01 00 07 ff f8 01 00 04 67 02 01 00 9f
STATUS 55 aa 00 02 00 06 ff f9 01 00 03 60 06 00 99
POLL   55 aa 00 05 00 05 ff fa 01 00 02 60 cb 52
poll d0 55 aa 00 07 00 05 ff fa 01 00 02 60 d0 4b
poll c1 55 aa 00 08 00 05 ff fa 01 00 02 60 c1 55
poll c0 55 aa 00 09 00 05 ff fa 01 00 02 60 c0 55
poll c9 55 aa 00 0a 00 05 ff fa 01 00 02 60 c9 5f
SETUP2 55 aa 00 0d 00 07 ff f8 01 00 04 67 02 01 00 93
SETUP5 55 aa 00 0f 00 07 ff f8 01 00 04 60 05 07 01 96
CONFIG 55 aa 00 10 00 08 ff f7 01 00 05 60 01 00 8c 31 37
TIMING 55 aa 00 11 00 0b ff f4 01 00 08 60 02 01 04 03 12 00 08 99
TIMEOUT 55 aa 00 12 00 0d ff f2 01 00 0a 60 03 00 3c 00 64 00 05 00 00 d8
DESCR 55 aa 00 13 00 0a ff f5 01 00 07 60 08 01 07 28 07 ff 54
BLOCK  55 aa 00 14 00 17 ff e8 01 00 14 60 09 03 01 10 0b 08 07 20 30 00 00 00 00 00 00 00 ff 00 00 6e
RECORD 55 aa 00 15 00 13 ff ec 01 00 10 61 04 00 0b 08 07 20 02 10 01 00 00 00 00 00 ff 56
mode01 55 aa 00 29 00 13 ff ec 01 00 10 61 04 00 0b 08 07 df 02 01 00 00 00 00 00 00 14 6e
mode09 55 aa 00 2c 00 13 ff ec 01 00 10 61 04 00 0b 08 07 df 02 09 02 00 00 00 00 00 14 61
freeze 55 aa 00 8e 00 13 ff ec 01 00 10 61 04 00 0b 08 07 df 03 02 0c 00 00 00 00 00 ff 2c
mode07 55 aa 00 ad 00 13 ff ec 01 00 10 61 04 00 0b 08 07 df 01 07 00 00 00 00 00 00 ff 04
mode0A 55 aa 00 ae 00 13 ff ec 01 00 10 61 04 00 0b 08 07 df 01 0a 00 00 00 00 00 00 ff 0a
D100   55 aa 00 78 00 13 ff ec 01 00 10 61 04 00 0b 08 07 e0 03 22 d1 00 00 00 00 00 ff 18
19-8F  55 aa 00 7b 00 13 ff ec 01 00 10 61 04 00 0b 08 07 e0 03 19 02 8f 00 00 00 00 ff 7c
3E00   55 aa 00 48 00 13 ff ec 01 00 10 61 04 00 0b 08 07 e0 02 3e 00 00 00 00 00 00 ff e4
14clear 55 aa 00 c5 00 13 ff ec 01 00 10 61 04 00 0b 08 07 e0 04 14 ff ff ff 00 00 00 ff ba
```

1004 session samples (host data payloads; the wire header is identical in format):

```
mode01 0C   01 00 10 61 04 00 0b 08 07 df 02 01 0c 00 00 00 00 00 01
mode21 LID01 01 00 10 61 04 00 0b 08 07 e0 02 21 01 00 00 00 00 00 ff
mode21 C1  01 00 10 61 04 00 0b 08 07 e0 02 21 c1 00 00 00 00 00 ff
mode09 VIN  01 00 10 61 04 00 0b 08 07 e0 02 09 02 00 00 00 00 00 fc
control 30  01 00 10 61 04 00 0b 08 07 e0 04 30 01 00 8a 00 00 00 ff
release 30  01 00 10 61 04 00 0b 08 07 e0 01 30 00 00 00 00 00 00 ff
mode06 35   01 00 10 61 04 00 0b 08 07 df 02 06 35 00 00 00 00 00 01
A8 01 list  01 00 10 61 04 00 0b 08 07 e0 02 a8 01 00 00 00 00 00 fa
A8 02 table 01 00 10 61 04 00 0b 08 07 e0 02 a8 02 00 00 00 00 00 fc
page 00     01 00 10 61 04 00 0b 08 07 e0 02 21 00 00 00 00 00 00 ff
50 c8       01 00 10 61 04 00 0b 08 07 50 c8 01 3e 00 00 00 00 00 ff
```

### 16.3 Repo assets worth copying (data, not protocol)

* `assets/brand_profiles.json` — brand behavior profiles (§12).
* `assets/brand_modules.json` — per-module objects + observed DIDs for Ford (0925).
* `assets/local_ids.json` — the 12-row Toyota mode-21 probe table (§10.5).
* `DidCatalogData.kt` / `PidCatalogData.kt` — generated catalogs (701 DIDs / 165 PIDs) if you want to
  reuse them; their generators are `tools/build_did_catalog.py` / `tools/build_pid_catalog.py`.

---

*Compiled 2026-10-04 from the captures and the working app in this repository. Anything marked [UNCLEAR]
was left deliberately unstated rather than guessed — the protocol is silent-failing, so an unverified
byte in a client is worse than a missing feature.*
