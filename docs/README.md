# Documentation map

Use this page to find the current instructions and distinguish active guidance from historical records.

## Current source of truth

- [README](../README.md): product overview, prerequisites, build commands, and project links.
- [CONTRIBUTING](../CONTRIBUTING.md): contributor onboarding, tests, documentation workflow, and license status.
- [AGENTS.md](../AGENTS.md): repository-wide engineering, safety, MCP parity, and data rules. More specific `AGENTS.md` files apply within their folders.
- [PROJECT_STATUS.md](../PROJECT_STATUS.md): current implementation, recent verification, hardware evidence links, and next validation work.
- [Agent Access / MCP](AGENT_ACCESS.md): setup, authorization, available tools, and current MCP behavior.
- [OEM signal browser](OEM_BROWSER.md): compact results, numeric-value and queue views, graph companion inspector, and grouped controls.
- [CAN discovery and diagnostics](DISCOVERY_DIAGNOSTICS.md): user-facing CAN and diagnostic workflows.
- [Vehicle/ECU profiles and diagnostic history](VEHICLE_PROFILES.md): offline VIN decoding, provenance-aware identity, explicit Resume previews, verified/paged report history, resumable import, and matching MCP operations.
- [Feature backlog](FEATURE_BACKLOG.md): deferred ideas only; not an active work assignment.
- [Licensing policy proposal](licensing-policy-proposal.md): the owner's no-paid-resale goals and the distinction between source-available and open-source licensing; this is not an active license.

## Architecture and research

- [Blueprint guide](../LogNook_Blueprint/README.md) indexes the product, architecture, protocols, storage, validation, security, references, and known vehicles documents.
- Architecture and product documents describe contracts or goals. Read `PROJECT_STATUS.md` before assuming a proposal is implemented.
- `PIDs/LogNook_PID_Master_v0_3` is a separate research-data project with its own agent instructions, schema rules, source provenance, and licensing policy.

## Historical records

- `hardware/` contains dated adapter/vehicle reports. Each report describes observations from its stated date and machine; it is evidence, not a guarantee for another adapter, vehicle, driver, or code revision.
- `superpowers/plans/` and `superpowers/specs/` preserve the scope and decisions for completed or abandoned work. Check source and project status before treating an unchecked item as current.
- `results/` and `Work to do by Ext Agent/` contain test outputs and external-agent reports. They are not current code instructions.

When a current user guide conflicts with code, verify the live implementation and update the guide alongside the behavior change. Keep old reports dated and append corrections or follow-up evidence rather than rewriting what was observed at the time.

## Documentation maintenance

Every feature change should update the current user/developer guide, architecture notes, and `PROJECT_STATUS.md` as applicable. MCP parity is required for capabilities with meaningful data or operations: update its typed request/schema and tool description, keep authorization and the existing serialized app owner, and test the exposed behavior. Record a brief UI-only rationale for features with no meaningful MCP equivalent.
