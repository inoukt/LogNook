# Diagnostic signal presets

Signal presets are recipes for browsing and selecting canonical diagnostic
concepts. They are not vehicle identities and do not turn a research mapping
into a verified request. Built-ins are shipped immutable recipes; user recipes
are stored with vehicle profiles in `profiles.sqlite` and may be copied and
edited without changing the authoritative PID catalog.

Generic recipes can be browsed without a vehicle. Model and exact-profile
recipes require matching context. One unique exact saved-profile match can
prefill profile context; saved owner-entered make/model/year/powertrain details
are used when present. Manually entered context is a filter only; it does not
prove ECU compatibility. This release passes no exact ECU evidence to preset
resolution, so all OEM alternatives remain skipped. Before future OEM preset
Apply support, the backend must supply current observed vehicle evidence, the
connection generation and exact candidate source/decoder revision.

An accepted selection change uses the existing acquisition owner and its
acknowledged signal replacement. Add deduplicates the selected IDs; Replace
uses the selection shown in the preview. Recording, playback and conflicting
adapter work prevent Apply. Browsing and preview are local catalog operations
and do not transmit vehicle requests.

The catalog currently supplies eight immutable built-in recipes. They contain
standard OBD channel suggestions only; this release does not include invented
model-specific OEM mappings. `not_tested` and community research definitions do
not count as reviewed applicability or physical verification. No physical
vehicle validation was performed as part of preset implementation.

Custom recipes preserve logging and graph selections separately. Saving a
recipe copies selection intent; unsupported signals are resolved as unavailable
when a fresh vehicle preview is prepared. The saved snapshot retains separate
log/graph lists, derived definitions, CAN fields and OEM references. The current
preset apply path accepts only supported generic OBD signals; derived and CAN
channels remain unavailable until their profile validation paths are integrated.
Offline preview shows the staged concept list and marks support unknown; it does
not claim that the connected ECU supports any channel. Selecting another preset
replaces the staged intent and advances the picker revision for subsequent
selections.

Preset priorities and slow-context labels are stored as recipe metadata. The
existing acquisition scheduler does not yet budget per-signal rates from these
fields; shown rates continue to be measured from actual adapter samples.
