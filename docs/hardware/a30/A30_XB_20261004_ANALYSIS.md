# A30 ↔ 2014 Scion xB — analysis of `btsnoop_hci_20261004130-.cfa`

One self-contained file: the complete walk-through of the 2026-10-04 capture — what was done on the
car, every decoded exchange, and the protocol model that makes it readable. All numbers here were
re-derived from the capture bytes (not copied from an earlier doc).

Canonical protocol reference: **`A30_PROTOCOL_EXPORT.md`** (same facts, organised as a spec for a new
client). This file is the session narrative and the evidence tables; that file is the reference.

---

## 0. The capture

| | |
|---|---|
| File | `btsnoop_hci_20261004130-.cfa` (Android HCI snoop, LS btsnoop) |
| Duration | ~338 s |
| Link | SPP/RFCOMM, handle `0x13`, DLCI 12 (the A30 data channel) |
| Dongle | `A30M-111948` — identity blob matches the 0915/0921 captures |
| Frames | **993 host → dongle**, **1979 dongle → host**, **every trailing XOR valid** |
| Vehicle | **2014 Scion xB (AZE151)**, VIN `JTLZE4FE1EJ049314`, engine **2AZ-FE** (read from the car) |
| App | not ours — a more capable diagnostic app (it implements shapes our client does not) |

Three things were done on the car, in this order: a **live-data scan** (manufacturer mode-21 stream +
a full OBD mode-01 sweep), **mode 06** monitor tests, and **live actuations**.

---

## 1. The record layer is CAN — the key finding

The dongle is a CAN bridge. The 13 bytes after a request's 6-byte preamble are a classic-CAN frame
plus its length:

```
01 00 10 61 04 00 | 0b | 08 | 07 e0 | 02 21 01 00 00 00 00 00 | ff
   fixed preamble   len  DLC   CAN id   8 CAN data bytes          trailer (dongle directive)
```

* `0b` = u8 length (11 = DLC + id + 8 data); `08` = DLC; `07 e0` = **CAN id 0x7E0**.
* What our docs called `obj` is the CAN id's low byte:
  `e0` = **0x7E0** (ECM), `df` = **0x7DF** (functional OBD), `c0` = **0x7C0** (second module),
  `50` = 0x750 (one unanswered probe).
* What our docs called `slot` is the **ISO-TP PCI**: `01`–`07` = single frame of that many bytes
  (`02 21 01` = payload `21 01`; `04 30 01 00 8a` = payload `30 01 00 8a`); `30 00 …` = flow control.
* Replies carry the **response** CAN id, not the request's: `07 e8` (0x7E8, the ECM — it also answers
  the functional 0x7DF) and `07 c8` (0x7C8). Physical request id + 8 = response id.

Reply shapes:

```
FFF1  01 | 00 0b | 08 | 07 e8 | <PCI> <answer bytes> <00 pad>              (one CAN record)
block <count> + count × (00 0b 08 07 <obj> <PCI> <7 CAN bytes>)            (13-byte records)
      command low byte encodes the count:  low = (0xF1 - 13*(n-1)) mod 256
      FFF1=1, FFE4=2, FFD7=3, FFCA=4, FFBD=5, FF6F=11, FF55=13   (all verified)
long  00 <u16 BE len> 00 07 <obj> 10 <innerlen> <innerlen bytes>           (FA4C / FF4F)
```

Two more consequences:

* **`FFE8` BLOCK sends the ISO-TP flow-control frame** `30 00 00 00 00 00 00 00` — that is what lets a
  multi-frame answer (VIN, 21-byte id, 28-byte live block) complete. The dongle repeats it as a stream.
* One block can carry **several complete answers** (the 1004 `FF6F` holds a 53-byte and a 14-byte
  message) — split on every PCI `10`.

---

## 2. Session timeline

| Time | Phase |
|---|---|
| 0–18 s | open sequence (SETUP, STATUS ×3, poll sweep, config burst) — standard |
| 23.5–38 s | **ECM identify**: `3e 00`, `21 c1`, `21 28`, `09 02` (VIN), `21 e0/e2`, page sweep, `a8 01` |
| 39.6–89.6 s | **live stream**: `21 01` at 4.8 Hz, `21 e1/e3/c1` alongside |
| 95.2 s | `a8 02` table on `e0` |
| 118.4–127.3 s | **actuation sweep** (`30 01 00 v` ×13) then `30 00` release |
| 134.7–136.8 s | `50 c8 01 3e` probe (CAN 0x750, unanswered) |
| 146–151 s | **second module** `c0`/0x7C0: `3e 00`, `21 e2`, `a8 02`, `21 e0`, sweep, `a8 01` |
| 166.8–174.9 s | `FFDA` multi-frame write to 0x7C0 ×2; `30 00` release on `c0` |
| 193.5–205.2 s | **mode-01 full scan**: bitmaps + every advertised PID |
| 205.4–228.4 s | `01 0C` (RPM) stream, 4 Hz |
| 233.2–247.8 s | **mode 06**: bitmaps + TIDs 35, A2, A5 |
| 248–338 s | tail (keepalive/poll cadence only) |

---

## 3. ECM identification (23.5–38 s, all on `e0` / CAN 0x7E0)

| Request | Answer | Notes |
|---|---|---|
| `3e 00` | `7e` (1 byte) | tester present **accepted** (our own earlier xB session got `7F 3E 12` — unexplained, see §8) |
| `21 c1` | 21 B `61 c1 41 5a 45 31 35 31 20 32 41 5a 46 45 20 04 02 41 71 01 00` | ASCII **"AZE151 2AZFE"** (chassis + engine) + `04 02 41 71 01 00` |
| `21 28` | `7f 21 12` | subFunctionNotSupported — a named refusal |
| `09 02` | `49 02 01` + `JTLZE4FE1EJ049314` | VIN read on the **physical** channel (our 0915 doc used `df`) |
| `21 e2` | `61 e2 f7 f7 d9 f3 70` | |
| `21 e0` | `61 e0 e0 00 00 00` | (`c0` answers `61 e0 c0 00 00 00` — echoes the CAN id) |
| `21 00/20/40/60/80/a0/c0` | `61 00 bc 00 00 01`, `61 20 ac 00 22 d1`, `61 40 9c 00 d0 01`, `61 60 00 00 00 01`, `61 80 c8 00 00 01`, `61 a0 d3 f9 c0 01`, `61 c0 fe 80 80 01` | page sweep — looks like "which LIDs exist per 0x20 page"; it is how the app picked `c1/e1/e3` |
| `a8 01` | **26 long records, 1459 bytes** in one `FA4C` frame | the supported-identifier list, entries shaped `<id> <len> <len bytes>` |

---

## 4. Live data stream (39.6–89.6 s)

| LID | Reads | Answer | Transport |
|---|---|---|---|
| `01` | **424** (4.79 Hz) | 28 B: `61 01` + 26 live bytes, changes every sample | FFBD |
| `e1` | 56 (1.72 Hz) | 10 B: `61 e1 01 01 0c 46 20 00 02 0a`; last byte rises steadily (counter/timer) | FFE4 |
| `e3` | 112 (3.45 Hz) | 4 B: `61 e3 00 00` | FFF1 |
| `c1` | 57 (~1 Hz) | the 21-byte id string above | FFCA |

Measured mode-21 gap: **min 34 ms, mean ≈83 ms**; 646 mode-21 reads on `e0` (+10 on `c0`).

One exact `21 01` reassembly (the live block, 28 bytes):

```
FFBD: 05 00 0b 08 07 e8 10 1c 61 01 00 00 00 00 00 · 0b 08 07 e8 21 25 47 65 85 00 00 00 00 ·
      0b 08 07 e8 22 00 00 00 30 82 29 52 00 · 0b 08 07 e8 23 28 20 01 38 02 33 2f 00 ·
      0b 08 07 e8 24 c2 00 00 00 00 00 00
PDU: 61 01 | 00 00 00 00 | 25 47 65 85 00 00 00 | 00 00 00 30 82 29 52 | 28 20 01 38 02 33 2f | c2
```

At 95.2 s the app also read `a8 02` on `e0`: a 53-byte `e8 02` table (delivered in an `FF6F` block);
the same read on `c0` returned 62 bytes (`FF55`).

---

## 5. Mode-01 full scan (193.5–228.4 s, `df` / CAN 0x7DF, PCI 02)

All four supported-PID bitmaps were read (the xB advertises PIDs into the 0x60 block):

```
41 00 be 1f a8 13    01,03,04,05,06,07,0C,0D,0E,0F,10,11,13,15,1C,1F,20
41 20 90 05 b0 15    21,24,2E,30,31,33,34,3C,3E,40
41 40 fa dc a0 01    41,42,43,44,45,47,49,4A,4C,4D,4E,51,53,60
41 60 08 00 00 00    65
```

(0x60 is the "supported 61–80" PID itself; the only value PID there is `65`.)

The app then polled **every advertised PID once, in descending order**:

```
65 53 51 4E 4D 4C 4A 49 47 45 44 43 42 41 3E 3C 34 33 31 30 2E 24 21 1F 1C 15 13 11 10
0F 0E 0D 0C 07 06 05 04 03 01
```

then streamed `01 0C` (RPM) for 23 s at 4 Hz (93 reads). Two PIDs that are **not** in the bitmaps
(`01 A6`, `01 D3`) were probed first and stayed silent — a useful negative control. Total mode-01
poll count: **199**.

---

## 6. Mode 06 (233.2–247.8 s, `df` PCI 02, trailer `01`)

TID bitmaps (`46 <tid>` + 4 bytes, 0x20 TIDs per block):

| Request | Answer | Advertises |
|---|---|---|
| `06 00` | `46 00 c0 00 00 01` | TIDs 01, 02, 20 |
| `06 20` | `46 20 80 00 08 09` | 21, 35, 3D, 40 |
| `06 40` | `46 40 40 00 00 01` | 42, 60 |
| `06 60` | `46 60 08 00 00 00` | 65 |
| `06 80` | `46 80 00 00 00 00` | none |
| `06 a0` | `46 a0 f8 00 00 00` | A1–A5 |

TID reads — result format `46 <tid>` + N × 9 bytes
`<tid> <MID> <UAS> <value u16 BE> <min u16 BE> <max u16 BE>`:

```
06 35 -> 46 35 81 35 00 00 00 00 03 b5
         MID 81, UAS 35, value 0, min 0, max 949
06 a2 -> 46 a2 0b 24 00 00 00 00 ff ff a2 0c 24 00 01 00 00 ff ff
         MID 0B value 0 / MID 0C value 1, min 0, max 65535
06 a5 -> 46 a5 0b 24 00 00 00 00 ff ff a5 0c 24 00 00 00 00 ff ff
         same shape, both values 0
```

---

## 7. Live actuations (118.4–127.3 s, `e0` / CAN 0x7E0)

Thirteen `30 01 00 <v>` writes (PCI 04) ramped the argument up, then back down:

```
118.44 8a   121.48 94   122.06 9e   122.42 a8   122.78 b2
123.57 a8   123.90 9e   124.21 94   124.55 8a   125.03 80
125.62 76   125.97 6c   126.32 62
```

each answered **`70 01`** (2 bytes). At 127.31 s, one `30 00` (PCI 01) answered **`70`** (1 byte) —
the release. This is an input-output control (`30`) on local identifier 01: *set* then *release*.
Which actuator it drove is not visible in the bytes.

---

## 8. Other traffic worth knowing

* **Second module** at 146–151 s: everything repeated on `c0` (CAN 0x7C0), answers tagged `c8`
  (0x7C8). This is what proved the request/response CAN-id relationship.
* **The only vehicle write in the capture** — `FFDA` at 166.8/170.6 s: `<count=02>` + two 18-byte
  records forming one ISO-TP message (`10 0c` first frame + `21` continuation) whose 12-byte payload
  `30 0b 00 00 00 00 40 00 00 00 00 00` went to 0x7C0. Meaning unknown.
* **Timer record** `FFEA` (19–22 s): the app installed the `3e 00` keepalive into the dongle
  (`88 18 da <00|5a>` wrapper) and stopped sending it itself.
* **New commands seen**: `FFE6` (block variant), `FFEA` (timer record), `FFDA` (host multi-frame
  write), `FF4F`/`FA4C` (long-record replies).
* **`50 c8 01 3e`** (134.7, 136.8 s): the only request whose first data byte (`c8`) is not an ISO-TP
  PCI; CAN 0x750; no answer.
* **The 3E contradiction**: this session got `7e` for `e0 02 3e 00`; our own earlier xB session got
  `7F 3E 12` for a 3E. Not yet explained (trailer? session order? PCI?) — see
  `A30_PROTOCOL_EXPORT.md` §14.7.

---

## 9. Verified vs. still open

**Verified from this capture** (all in `A30_PROTOCOL_EXPORT.md` with the same evidence tags):

* the CAN-frame model of the record layer (§2.1 there);
* the block-count formula `low = (0xF1 − 13·(n−1)) mod 256`, n = 1…5, 11, 13;
* the long-record family and the 1459-byte `a8 01` transfer;
* mode-06 bitmaps and the 9-byte result format;
* the `30 01 00 v` → `70 01` actuation shape and `30 00` → `70`;
* `a8 01`/`a8 02` sizes; `FFE8` = flow-control stream; `FFDA` = host multi-frame write.

**Still open**: the field map of the 28-byte `21 01` block; `a8 01/02` field semantics; the page
bitmaps' bit order; the `50 c8` channel; which actuator `30 01 00 v` drives; the exact `FFEA` timer
field; and the 3E contradiction above.

---

## 10. Reproducing these numbers

```python
import sys; sys.path.insert(0, 'tools')
from btsnoop import read_records
import snoop_tail as S
datalink, recs = read_records('btsnoop_hci_20261004130-.cfa')
# keep handle 0x13 / DLCI 12, strip the RFCOMM credit octet when ctrl & 0x10,
# then S.decode(...) yields Frame(cmd, data) for both directions.
```

* Host requests: `cmd == 0xFFEC`, `data[9]` = CAN id low byte, `data[10]` = PCI, `data[11:18]` = the
  8 CAN data bytes, `data[18]` = trailer.
* Replies: `cmd == 0xFFF1` is one record; `0xFFE4`/`FFD7`/`FFCA`/`FFBD`/`FF6F`/`FF55` are record
  lists (first byte = count); `0xFA4C`/`0xFF4F` are long records (walk `pos += 7 + p[pos+6]`).
* The capture's own checksum proof: recompute the XOR for every frame — **0 mismatches** in both
  directions.

*Written 2026-10-04 from the capture bytes. Companion: `A30_PROTOCOL_EXPORT.md` (the protocol spec),
`A30PROTOCOL.md` (the working app spec), `tools/` (the analysis scripts).*
