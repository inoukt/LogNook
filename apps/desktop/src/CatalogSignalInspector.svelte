<script lang="ts">
  import type { CatalogInspection } from "./catalogInspection";
  export let inspection: CatalogInspection;
  export let onClose: () => void = () => {};
</script>

<aside class="catalog-inspector" aria-label="Selected OEM signal details">
  <header>
    <div class="identity">
      <span class="eyebrow">SELECTED SIGNAL</span>
      <h3>{inspection.name}</h3>
      <p>{inspection.subtitle}</p>
    </div>
    <div class="result" class:has-value={inspection.tone === "value"} class:warning={inspection.tone === "warning"}>
      {#if inspection.value}<strong>{inspection.value}</strong>{/if}
      <span>{inspection.status}</span>
    </div>
    <button class="close-inspector" type="button" aria-label="Hide selected signal details" title="Hide signal details" onclick={onClose}>×</button>
  </header>
  <div class="inspector-body">
    <dl>
      {#each inspection.fields as field}<dt>{field.label}</dt><dd>{field.value || "—"}</dd>{/each}
    </dl>
    <div class="inspector-actions">
      {#each inspection.actions as action}
        <div class="inspection-action">
        <button class="button secondary" type="button" disabled={action.disabled} title={action.title} onclick={() => action.run()}>{action.label}</button>
        {#if action.disabled && action.title}<small>{action.title}</small>{/if}
        </div>
      {/each}
      {#if inspection.note}<p>{inspection.note}</p>{/if}
    </div>
  </div>
  {#if inspection.blocks?.length || inspection.links?.length}
    <details class="source-details">
      <summary>Decoder, applicability and source</summary>
      <div class="source-content">
        {#each inspection.blocks ?? [] as block}
          <details><summary>{block.label}</summary><pre>{block.value}</pre></details>
        {/each}
        {#each inspection.links ?? [] as link}<a href={link.url} target="_blank" rel="noreferrer">{link.label} ↗</a>{/each}
      </div>
    </details>
  {/if}
</aside>

<style>
  .catalog-inspector { flex: 0 1 auto; min-height: 0; max-height: 38%; overflow: auto; padding: 11px 13px; border: 1px solid var(--border-strong); border-radius: 8px; background: var(--surface); scrollbar-gutter: stable; }
  header { position: sticky; top: 0; z-index: 1; display: flex; align-items: flex-start; gap: 12px; padding-bottom: 5px; background: var(--surface); }
  .identity { flex: 1; min-width: 0; }
  .eyebrow { display: block; margin-bottom: 3px; font-size: 8px; letter-spacing: 1px; }
  h3 { margin: 0 0 3px; font-size: 14px; overflow-wrap: anywhere; }
  p { margin: 0; color: var(--text-muted); font-size: 10px; line-height: 1.4; overflow-wrap: anywhere; }
  .result { display: flex; flex-direction: column; align-items: flex-end; gap: 3px; color: var(--text-muted); font-size: 10px; }
  .result strong { color: var(--text); font-size: 19px; font-variant-numeric: tabular-nums; }
  .result.has-value, .result.has-value strong { color: var(--accent); }
  .result.warning { color: var(--status-warning, #e8bc69); }
  .close-inspector { width: 25px; height: 25px; padding: 0; flex: 0 0 auto; border: 1px solid var(--border); border-radius: 5px; background: transparent; color: var(--text-muted); cursor: pointer; font-size: 18px; }
  .close-inspector:focus-visible, summary:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .inspector-body { display: grid; grid-template-columns: minmax(0, 1fr) minmax(160px, .6fr); gap: 12px; margin-top: 9px; }
  dl { display: grid; grid-template-columns: 90px minmax(0, 1fr); gap: 4px 9px; margin: 0; font-size: 10px; }
  dt { color: var(--text-muted); }
  dd { margin: 0; overflow-wrap: anywhere; }
  .inspector-actions { display: flex; flex-wrap: wrap; align-content: start; gap: 5px; }
  .inspector-actions .button { min-height: 28px; padding: 3px 9px; font-size: 10px; }
  .inspection-action { display: grid; align-content: start; gap: 4px; min-width: 0; }
  .inspection-action small { max-width: 260px; color: var(--text-muted); font-size: 10px; line-height: 1.4; overflow-wrap: anywhere; }
  .inspector-actions p { flex-basis: 100%; }
  .source-details { margin-top: 9px; padding-top: 7px; border-top: 1px solid var(--border-soft); font-size: 10px; color: var(--text-muted); }
  summary { cursor: pointer; }
  .source-content { display: grid; gap: 7px; padding-top: 8px; }
  pre { max-height: 160px; overflow: auto; margin: 5px 0; padding: 7px; background: var(--surface-deep); white-space: pre-wrap; overflow-wrap: anywhere; font-size: 10px; }
  a { width: fit-content; color: var(--accent); }
  @media (max-width: 1050px) { .inspector-body { grid-template-columns: 1fr; gap: 8px; } }
  @media (max-width: 800px) { .catalog-inspector { max-height: 280px; flex-shrink: 0; } .result strong { font-size: 16px; } }
</style>
