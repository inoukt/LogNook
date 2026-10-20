# Research source register and licensing policy

Full machine-readable URLs and warnings: `data/sources.csv`.

## Official sources and boundaries

1. **SAE J1979 Digital Annex:** an August 2026 revision is visible in an SAE test/publication mirror, while the primary publisher listing showed July 2026 as current during research; confirm authoritative availability and acquire the correct licensed issue. Public publication details checked, but the complete licensed registry was **not acquired or reproduced**. The public Service 01 / 09 seed is not guaranteed exhaustive against that licensed revision. A compliance release needs an authorized copy and systematic cross-check with a standards change log.
2. **SAE J1939 Digital Annex:** separate heavy-duty parameter group/suspect parameter space. Do not insert J1939 SPNs as if they were OBD Mode 01 PIDs.
3. **ASAM ODX / A2L:** future OEM data import paths, subject to model-specific access and separate licenses.

## Community sources and packaging

- **OBDb**, https://github.com/OBDb, uses CC BY-SA 4.0 for much of its published data. Credit OBDb and its contributors, preserve source URLs/commit/revision, comply with ShareAlike where applicable, and do not assume every repo/asset has identical licensing. Full imported packs should be separately versioned, attributed and legally reviewed before redistribution.
- **RomRaider**, https://github.com/RomRaider/RomRaider, is published as GPL-2.0 in its repository. The linked `log_defs.xml` is used as a technical reference for the separate **SSM research candidate** examples. Confirm the specific definitions' provenance and redistribution compatibility with the intended LogNook license, or require the user to supply a separately obtained compatible definition file and import it without redistribution. Do not paste RomRaider application code into LogNook.
- **Wikipedia OBD-II PID table**, https://en.wikipedia.org/wiki/OBD-II_PIDs, is a public secondary reference subject to Wikipedia's applicable attribution/ShareAlike terms; preserve attribution for adapted content. Verify formulas and unusual extended entries against official documents before claiming compliance.
- **CSS Electronics** and **python-OBD** are secondary cross-check resources. Do not copy code or commercial documentation without satisfying individual licensing requirements.

## Quality review cases encountered

- Some Toyota OBDb rows are explicitly marked debugging-only and some duplicate descriptions expose inconsistent displayed unit/range annotations (e.g. "fuel pump duty ratio" with a pressure unit). Such entries were **excluded** from the curated sample or marked for manual review. Never blindly promote scraped data to production.
- BMW and newer Hyundai EV data can require special addressing and bit-endianness; source metadata must be preserved for accurate decoding.
- The Subaru base source includes a parameter labeled engine load with a unit annotation inconsistent with a percent-scaled expression. That ambiguous field was intentionally excluded from the curated SSM sample.
- SAE's digital annex update status may change; verify official licensed content prior to release.

**Redistribution decision:** treat v0.1 as an internal research seed. Ship only first-party validated generic code and properly licensed per-source data packs once legal and technical review are complete. The source catalog enables provenance but is not a legal opinion or a substitute for permission.

## Subaru OEM catalog research material

The local Subaru export retains its original package identity and revision in internal source metadata, but no source URL or reuse license was present in the supplied package. The database records `license_status='not_cleared'` and a SHA-256 of the local `catalog.json` export. That hash identifies the local file only; it is not a publisher checksum or upstream Git blob SHA. Keep the data local for research pending rights review, and do not redistribute it based on the database import alone.
