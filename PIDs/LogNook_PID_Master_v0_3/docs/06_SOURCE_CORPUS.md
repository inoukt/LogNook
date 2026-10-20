# Source Corpus — Canada Core

The v0.2 database does not pretend that a manufacturer-specific DID is universal. Instead, it
indexes upstream definition corpora and records exactly what scope each corpus claims.

## Make-level sources

| Family | Badge(s) | Indexed upstream signal rows |
|---|---|---:|
| Ford Motor Company | Ford, Lincoln | 970 |
| General Motors | Chevrolet, GMC, Buick, Cadillac | 2,590 |
| Toyota Motor Corporation | Toyota, Lexus | 2,779 |
| Honda Motor Company | Honda, Acura | 691 |
| Hyundai Motor Group | Hyundai, Genesis, Kia | 4,050 |
| Nissan Motor Corporation | Nissan, Infiniti | 1,455 |
| Mazda Motor Corporation | Mazda | 1,272 |
| Subaru Corporation | Subaru | 273 |
| Volkswagen Group | Volkswagen, Audi | 5,848 |
| Stellantis | Ram, Jeep, Dodge, Chrysler | 4,630 |
| Mitsubishi Motors | Mitsubishi | 337 |
| BMW Group | BMW, MINI | 526 |
| Mercedes-Benz Group | Mercedes-Benz | 27 |
| Volvo Cars | Volvo | 800 |
| **Subtotal: Canada-core raw badge-level rows** | **28 badges** | **26,248** |

These are raw upstream rows and contain overlap between badges and related platforms. They are not
a unique-signal count.

## Model-specific corpora currently indexed

The initial model layer includes Ford Flex, Taurus, Explorer, F-150; Scion xB; Toyota RAV4 and
Corolla; Lexus RX; Chevrolet Equinox; Honda CR-V; Hyundai Tucson; Kia Sportage; Mazda CX-5;
Subaru Crosstrek, Outback, Forester and WRX; Volkswagen Tiguan and Jetta; Ram 1500;
Jeep Grand Cherokee; Mitsubishi Outlander; BMW 3 Series; and Volvo XC60.

Those model repositories contribute **6,510 indexed upstream rows** in their current default
signalsets. Model rows can overlap with the make fallback and with each other.

## Project test vehicles

- 2011 Ford Flex 3.5 EcoBoost
  - The OBDb Flex `default.json` is currently empty.
  - Use the Ford fallback plus separately tagged Taurus/Explorer/F-150 research only as candidates.
  - Do not claim a related-model DID applies until the Flex responds correctly and the decoder is verified.
- 2014 Scion xB 2.4
  - The indexed Scion-xB default corpus contains 539 decoded upstream signal rows across 213 commands.
  - Year, ECU address, response layout and formula still require vehicle validation.
- Subaru
  - Use OBD/UDS definitions plus a separately licensed/managed SSM definition track.

## Subaru OEM catalog research extension

The local **Subaru OEM catalog** export contains 20,855 distinct monitor-table rows linked to 45,364 ECU-specific entries, spanning 7 separate protocol-family labels, 98 vehicle records, and 168 ECU records. The 10 unlinked table names and row counts are recorded as orphans; their decoder rows are not fabricated.

The local export checksum and source revision are recorded in `subaru_source_releases`. The original package metadata is preserved internally, but there is no source URL, upstream Git blob SHA, or reuse license in the export. `license_status='not_cleared'` is intentional. The package is local research material only pending rights review.

`request_hint` values are guesses, not communication-support ground truth. The imported rows remain search-only with `mapping_state='inferred'` and `auto_request=0`; do not use them as a verified request plan. Keep protocol families distinct, and keep invalid year ranges unresolved.

## Source integrity

Each corpus row stores the exact Git blob SHA observed during research. `sync_obdb.py` computes the
Git blob SHA of downloaded bytes and refuses a changed revision by default.

This makes changes auditable and prevents a project rebuild from silently changing the PID catalog.


## Secondary indexed coverage

Additional make-level corpora are indexed for:

- Porsche — 2,951 upstream signal rows
- Land Rover — 82
- Jaguar — 82
- Fiat — 1,158
- Alfa Romeo — 64
- Tesla — 0 in the current default OBDb signalset

This brings the make-level source registry to **34 corpora / 30,585 raw upstream signal rows**.
Tesla remains in the registry specifically so the absence of a populated default corpus is explicit;
LogNook must use a separately researched Tesla data source rather than inventing compatibility.
