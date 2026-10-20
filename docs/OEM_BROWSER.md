# OEM signal browser

The OEM tab keeps search results beside the live graph. On a narrow window the
browser and graph stack in one scrollable workspace. Select a signal's name to
open its inspector below the graph; selecting another name replaces the details.
The inspector's close button stays visible while its contents scroll, and
**Show details** reopens it.

## Finding signals

- Choose **Community catalog** or **Subaru OEM catalog**.
- Search by the fields supported by that catalog. **Filters** exposes additional
  vehicle, service or ECU filters; its badge counts active filters even when closed.
- Community **All** shows the paged search. **With values · this page** shows
  finite numeric responses on the current page, including zero. Raw and negative
  replies remain in All with their result badges.
- Community **Selected** shows the retained research queue across search pages.
  The checkboxes control that queue independently from the inspected row.

The inspector shows the selected definition, available response evidence and
existing actions. Expand **Decoder, applicability and source** for the full
definition and provenance. A response or a numeric value does not establish
vehicle compatibility or decoder validation.

## Reading and graphing

Community **Graph** adds the research channel to the graph without a request.
**Try once** performs the existing explicit read and updates its result badge,
inspector and numeric-value view. **Run selected**, visible-row discovery and
filtered discovery retain their existing connection, cancellation and evidence
rules. Completed run results appear below the inspector once a run exists.

Subaru **Add to Live** adds the selected definition to the Live picker. Select
its Log checkbox there to use the existing supported transport. **CAN Workbench**
prepares the source-backed request for an explicit workbench operation. Actions
that are unavailable show their reason in the inspector. See
[Subaru catalog](SUBARU_CATALOG.md) for decoder and protocol details.

## Controls and implementation

The toolbar groups graph View, Connection and Acquisition controls; saved files
remain in the top bar's Sessions group. Controls wrap on narrow windows using
their existing handlers and operation gates.

`OemCatalogPanel.svelte` and `SubaruCatalogPanel.svelte` emit the presentation-only
`CatalogInspection` model to `CatalogSignalInspector.svelte`. Selection does not
transmit or mutate catalog definitions. The existing typed MCP search, discovery,
one-shot read and logging tools expose the underlying data and operations; this
layout and inspector do not add a separate MCP capability.
