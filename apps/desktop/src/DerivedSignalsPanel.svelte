<script lang="ts">
  import {
    isAvailableDerivedSignalId,
    newDerivedSignalDefinition,
    type DerivedSignalDefinition,
    type DerivedSignalSource,
  } from "./derivedSignals";

  let {
    definitions,
    baseSignalIds = [],
    sourceSignal = null,
    currentValues,
    disabled = false,
    tareDisabled = false,
    onChange,
    onTare,
  }: {
    definitions: DerivedSignalDefinition[];
    baseSignalIds?: string[];
    sourceSignal?: DerivedSignalSource | null;
    currentValues: Record<string, number | undefined>;
    disabled?: boolean;
    tareDisabled?: boolean;
    onChange: (definitions: DerivedSignalDefinition[]) => void;
    onTare: (id: string) => unknown;
  } = $props();

  let idErrors = $state<Record<string, string>>({});

  function update(id: string, field: "id" | "name" | "expression" | "unit", value: string) {
    onChange(definitions.map((definition) => definition.id === id ? { ...definition, [field]: value } : definition));
  }

  function updateId(id: string, event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const nextId = input.value.trim();
    if (!isAvailableDerivedSignalId(nextId, id, definitions, baseSignalIds)) {
      idErrors = { ...idErrors, [id]: "Use a unique ID made from letters, numbers, and underscores." };
      input.value = id;
      return;
    }
    const { [id]: _previousError, ...remainingErrors } = idErrors;
    idErrors = remainingErrors;
    update(id, "id", nextId);
  }

  function addDefinition() {
    onChange([...definitions, newDerivedSignalDefinition(definitions, sourceSignal)]);
  }

  function tare(definition: DerivedSignalDefinition) {
    if (currentValues[definition.id] === undefined) return;
    onTare(definition.id);
  }

  function remove(id: string) {
    onChange(definitions.filter((definition) => definition.id !== id));
  }
</script>

<section class="derived-signals-panel" aria-label="Derived signals">
  <div class="derived-heading">
    <div><p class="eyebrow">CUSTOM SIGNALS</p><h3>Derived signals</h3></div>
    <button class="button secondary" type="button" onclick={addDefinition} disabled={disabled}>＋ Add</button>
  </div>
  <p class="derived-hint">Use signal IDs, numbers, parentheses, and operators <code>+ - * /</code>. Example: <code>coolant_temp * 1.8 + 32</code>. Set the output unit separately, such as <code>°F</code>.</p>
  {#each definitions as definition (definition.id)}
    <div class="derived-definition">
      <label>Signal id
        <input class="text-input" value={definition.id} oninput={(event) => { delete idErrors[definition.id]; }} onchange={(event) => updateId(definition.id, event)} aria-invalid={Boolean(idErrors[definition.id])} disabled={disabled} />
        {#if idErrors[definition.id]}<span class="derived-error" role="alert">{idErrors[definition.id]}</span>{/if}
      </label>
      <label>Name
        <input class="text-input" value={definition.name} oninput={(event) => update(definition.id, "name", event.currentTarget.value)} disabled={disabled} />
      </label>
      <label>Formula
        <input class="text-input formula-input" value={definition.expression} oninput={(event) => update(definition.id, "expression", event.currentTarget.value)} disabled={disabled} />
      </label>
      <div class="derived-unit-row">
        <label>Output unit
          <input class="text-input" value={definition.unit} oninput={(event) => update(definition.id, "unit", event.currentTarget.value)} disabled={disabled} />
        </label>
        <span class="derived-current">Now<br /><strong>{currentValues[definition.id]?.toFixed(2) ?? "—"} {definition.unit}</strong></span>
      </div>
      <div class="derived-actions">
        <button class="button secondary" type="button" onclick={() => tare(definition)} disabled={tareDisabled || currentValues[definition.id] === undefined} title="Use the current value as zero">Tare to zero</button>
        <button class="button secondary" type="button" onclick={() => remove(definition.id)} disabled={disabled}>Remove</button>
      </div>
    </div>
  {:else}
    <p class="derived-empty">No derived signals yet.</p>
  {/each}
</section>

<style>
  .derived-signals-panel { border-top: 1px solid var(--line, #26323a); margin: 1rem 0; padding-top: 1rem; }
  .derived-heading { align-items: center; display: flex; justify-content: space-between; }
  .derived-heading h3 { font-size: 1rem; margin: 0; }
  .derived-heading .eyebrow { margin: 0 0 .25rem; }
  .derived-hint, .derived-empty { color: var(--muted, #94a2ab); font-size: .78rem; line-height: 1.45; }
  .derived-hint code { color: var(--accent, #56c8bd); }
  .derived-error { color: var(--danger, #ff8d83); font-size: .72rem; }
  .derived-definition { border: 1px solid var(--line, #26323a); border-radius: .65rem; display: grid; gap: .65rem; margin-top: .7rem; padding: .75rem; }
  .derived-definition label { color: var(--muted, #94a2ab); display: grid; font-size: .72rem; gap: .25rem; }
  .formula-input { font-family: ui-monospace, Consolas, monospace; }
  .derived-unit-row { align-items: end; display: grid; gap: .6rem; grid-template-columns: minmax(0, 1fr) auto; }
  .derived-current { color: var(--muted, #94a2ab); font-size: .7rem; line-height: 1.5; padding-bottom: .35rem; text-align: right; }
  .derived-current strong { color: var(--text, #edf3f5); font-size: .78rem; }
  .derived-actions { display: flex; gap: .5rem; }
  .derived-actions .button, .derived-heading .button { font-size: .75rem; padding: .4rem .55rem; }
</style>
