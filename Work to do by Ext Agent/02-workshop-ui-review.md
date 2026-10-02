# Side quest: workshop UI review

**Goal:** Independently review the Milestone 1 desktop screen for clarity and technician usability before adding more UI features.

## Read first

- `PROJECT_STATUS.md`
- `LogNook_Blueprint/01_PRODUCT.md`
- `LogNook_Blueprint/02_ARCHITECTURE.md`
- `apps/desktop/src/App.svelte`
- `apps/desktop/src/app.css`

## Work

Run the app with the documented development command and inspect it at the default size and a narrow 760 px window. If desktop execution is unavailable, state that and review the source only. Assess:

- Can a technician identify simulated versus physical connection status immediately?
- Are connect, PID selection, record, marker, CSV export, and replay states easy to understand?
- Are values, units, measured rates, and stale/error conditions legible in a workshop?
- Are contrast, focus visibility, labels, and keyboard navigation adequate?
- What is the single highest-value UI fix before Milestone 2?

## Constraints

- Review only; do not edit source, run vehicle commands, or make physical-compatibility claims.
- Keep findings specific to observable behavior and prioritize by user impact.

## Deliverable

Write `results/workshop-ui-review.md` with a short summary, prioritized findings, and the viewport/runtime used.
