<script lang="ts">
  import { vehicleProjectName, type VehicleProject } from "./vehicleProjects";

  export let project: VehicleProject | null = null;
  export let busy = false;
  export let error = "";
  export let onLoad: () => void;
  export let onFresh: () => void;
  export let onClose: () => void;
</script>

<div class="vehicle-project-backdrop" role="presentation">
  <dialog class="vehicle-project-prompt" open aria-labelledby="vehicle-project-title">
    <p class="eyebrow">Vehicle project</p>
    <h2 id="vehicle-project-title">{project ? vehicleProjectName(project.profile) : "Vehicle identity unavailable"}</h2>
    {#if project}
      <p>Load this vehicle’s saved workspace, or start fresh with the supported generic signals.</p>
      <div class="actions">
        <button class="button primary" type="button" onclick={onLoad} disabled={busy}>Load last state</button>
        <button class="button secondary" type="button" onclick={onFresh} disabled={busy}>Start fresh</button>
      </div>
    {:else}
      <p>VIN identification was unavailable. Continue with normal generic logging or choose a saved project from Profiles.</p>
      <div class="actions"><button class="button secondary" type="button" onclick={onClose}>Continue generic</button></div>
    {/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </dialog>
</div>

<style>
  .vehicle-project-backdrop { position: fixed; inset: 0; z-index: 100; display: grid; place-items: end; padding: 20px; background: #0006; }
  .vehicle-project-prompt { width: min(420px, calc(100vw - 40px)); margin: 0; padding: 18px; color: var(--text); background: var(--surface-raised, #202a35); border: 1px solid var(--border); border-radius: 12px; box-shadow: 0 16px 48px #0008; }
  .vehicle-project-prompt .eyebrow { margin: 0 0 4px; color: var(--text-muted); font-size: 11px; text-transform: uppercase; letter-spacing: .08em; }
  .vehicle-project-prompt h2 { margin: 0 0 8px; font-size: 17px; }
  .vehicle-project-prompt p { color: var(--text-muted); font-size: 13px; }
  .vehicle-project-prompt .actions { display: flex; gap: 8px; }
  .vehicle-project-prompt .error { color: var(--danger, #ff7f86); }
</style>
