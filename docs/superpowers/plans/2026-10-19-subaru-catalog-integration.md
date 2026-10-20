# Subaru OEM catalog integration

Approved direction: include the owner-supplied catalog with maximum searchability,
source traceability and explicit manual selection. User-facing names use Subaru
OEM catalog. Source origin and physical verification remain separate; no imported
row silently becomes a validated automatic polling definition.

## Implementation

1. Luna xhigh importer: read the local data-monitor export, normalize repeated
   definitions and ECU/vehicle relations in the existing authoritative SQLite
   catalog. Preserve unknown applicability, source hashes, raw decoder fields
   and request-hint uncertainty. Add deterministic/idempotence tests; run both
   existing database suites. Back up the local database before import.
2. Luna xhigh app integration: query the normalized `subaru_catalog_search` view
   through one Rust service. Expose paginated search/details in UI and typed MCP.
   Show protocol, route, source revision, decoder and validation status. Preserve
   unsupported rows as searchable references rather than claiming compatibility.
3. Parent integration: add a Subaru catalog tab to OEM search; prepare an exact
   Mode 21/22 CAN Workbench test from eligible selections. Never transmit during
   selection. Existing operator-controlled Workbench execution, cancellation and
   raw evidence remain the request owner. Unsupported transports are identified.
4. Review and verification: review import fidelity, protocol separation,
   eligibility and lookup bounds; run relevant database, Rust and UI checks,
   update architecture/progress/MCP docs and build the local release.

## Constraints

- Existing uncommitted application changes and source pack remain intact.
- No hardware requests, driver/firmware changes, commit or push in this task.
- Raw source product metadata remains internal; public naming is neutral.
- Missing source licensing metadata is preserved honestly. No fabricated license
  or source URL is used to bypass existing OBD candidate gates. This local import
  does not establish redistribution rights for the extracted source data.
- Normal graph/recording values require actual decoded responses. Request hints
  alone are not measurements or verified vehicle compatibility.
- Preserve all measurement definitions, with aliases represented by links; do not
  import active tests, customization or programming material as measurements.

## Ownership / progress

Importer owns Python/schema/database tests and local database import. App agent
owns the new Rust service, typed commands/MCP and standalone Subaru panel.
Parent owns existing OEM wrapper, App/Workbench handoff and project docs. These
files are disjoint; the shared interface is the SQLite view and typed row.

Parent handoff tests: three passed. Import and app work in progress.
