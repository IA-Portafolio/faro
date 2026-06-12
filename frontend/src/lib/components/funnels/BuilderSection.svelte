<script lang="ts">
  import { windowPresets } from '$lib/funnels';

  export let funnel: string[];
  export let windowSecs: number;

  export let onWindowChange: (s: number) => void;
  export let onClear: () => void;
  export let onRemoveStep: (i: number) => void;
  export let onDragStart: (e: DragEvent, i: number) => void;
  export let onDragOver: (e: DragEvent) => void;
  export let onDropAt: (e: DragEvent, i: number) => void;
  export let onDropEnd: (e: DragEvent) => void;
</script>

<section class="pane builder">
  <div class="builder-head">
    <h2 class="pane-title">Construcción</h2>
    <div class="builder-controls">
      <label class="window-label">
        <span class="muted">Ventana</span>
        <select
          value={windowSecs}
          on:change={(e) => onWindowChange(Number((e.currentTarget as HTMLSelectElement).value))}
        >
          {#each windowPresets as p}
            <option value={p.seconds}>{p.label}</option>
          {/each}
        </select>
      </label>
      {#if funnel.length > 0}
        <button type="button" class="ghost" on:click={onClear}>Limpiar</button>
      {/if}
    </div>
  </div>

  {#if funnel.length === 0}
    <div
      class="dropzone empty"
      role="region"
      aria-label="Zona para soltar eventos"
      on:dragover={onDragOver}
      on:drop={(e) => onDropAt(e, 0)}
    >
      Arrastrá un evento acá para empezar.<br>
      Mínimo 2 pasos para ver resultados.
    </div>
  {:else}
    <ol class="funnel-steps">
      {#each funnel as ev, i (i + ':' + ev)}
        <!-- Drop-zone ANTES de este paso (insertar en posición i) -->
        <li
          class="drop-gap"
          on:dragover={onDragOver}
          on:drop={(e) => onDropAt(e, i)}
          aria-hidden="true"
        ></li>
        <li
          class="step"
          draggable="true"
          on:dragstart={(e) => onDragStart(e, i)}
        >
          <span class="step-index mono">{i + 1}</span>
          <span class="step-name">{ev}</span>
          <button
            type="button"
            class="step-remove"
            on:click={() => onRemoveStep(i)}
            aria-label={`Eliminar paso ${i + 1}`}
            title="Eliminar"
          >×</button>
        </li>
      {/each}
      <!-- Drop-zone al final -->
      <li
        class="drop-gap last"
        on:dragover={onDragOver}
        on:drop={onDropEnd}
        aria-hidden="true"
      ></li>
    </ol>
  {/if}
</section>

<style>
  .builder-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .builder-controls { display: flex; gap: 8px; align-items: center; }
  .window-label { display: flex; gap: 6px; align-items: center; font-size: 12px; }
  .ghost {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-muted);
    font-size: 12px;
    padding: 4px 10px;
  }
  .ghost:hover { color: var(--text); }

  .dropzone {
    border: 2px dashed var(--border);
    border-radius: 8px;
    padding: 40px 16px;
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.6;
  }
  .dropzone.empty { background: transparent; }

  .funnel-steps {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
  }
  .step {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    cursor: grab;
  }
  .step:active { cursor: grabbing; }
  .step-index {
    width: 22px; height: 22px;
    border-radius: 50%;
    background: var(--accent);
    color: var(--accent-fg);
    font-size: 11px;
    font-weight: 600;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .step-name { flex: 1; font-size: 13px; overflow: hidden; text-overflow: ellipsis; }
  .step-remove {
    background: transparent;
    border: 0;
    color: var(--text-muted);
    font-size: 18px;
    line-height: 1;
    cursor: pointer;
    padding: 0 6px;
  }
  .step-remove:hover { color: var(--danger); }
  .drop-gap {
    height: 8px;
    border-radius: 4px;
    transition: background 80ms;
  }
  .drop-gap.last { height: 16px; }
  .drop-gap:hover { background: var(--bg-hover); }
</style>
