<script lang="ts">
  import { helpOpen } from '$lib/keyboard';

  type Row = { keys: string[]; label: string };
  type Group = { title: string; rows: Row[] };
  type JumpRow = { syntax: string; label: string };

  const jumps: JumpRow[] = [
    { syntax: 'traces:<id>', label: 'Abrir la traza' },
    { syntax: 'logs:trace=<id>', label: 'Ver logs de la traza' },
    { syntax: 'errors:<fingerprint>', label: 'Abrir el issue' }
  ];

  const groups: Group[] = [
    {
      title: 'Navegación',
      rows: [
        { keys: ['g', 'r'], label: 'Ir a Resumen' },
        { keys: ['g', 'l'], label: 'Ir a Logs' },
        { keys: ['g', 't'], label: 'Ir a Trazas' },
        { keys: ['g', 'm'], label: 'Ir a Métricas' },
        { keys: ['g', 'e'], label: 'Ir a Errores' },
        { keys: ['g', 'o'], label: 'Ir a Monitores' },
        { keys: ['g', 's'], label: 'Ir a Configuración' },
        { keys: ['g', 'a'], label: 'Ir a Alertas' },
        { keys: ['g', 'p'], label: 'Ir a Proyectos' },
        { keys: ['g', 'u'], label: 'Ir a Usuarios' },
        { keys: ['g', 'i'], label: 'Ir a Integraciones' }
      ]
    },
    {
      title: 'Global',
      rows: [
        { keys: ['⌘', 'K'], label: 'Abrir paleta de comandos (Ctrl+K en Windows/Linux)' },
        { keys: ['?'], label: 'Mostrar esta ayuda' },
        { keys: ['Esc'], label: 'Cerrar diálogo / paleta / drawer' }
      ]
    },
    {
      title: 'Listas (Logs, Trazas, Errores)',
      rows: [
        { keys: ['/'], label: 'Enfocar el campo de búsqueda' },
        { keys: ['j'], label: 'Bajar a la siguiente fila' },
        { keys: ['k'], label: 'Subir a la fila anterior' },
        { keys: ['↵'], label: 'Abrir la fila resaltada' },
        { keys: ['Esc'], label: 'Quitar selección / cerrar detalle' }
      ]
    }
  ];

  function close(): void {
    helpOpen.set(false);
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
    }
  }
</script>

{#if $helpOpen}
  <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
  <div class="help-backdrop" role="presentation" on:click={close} on:keydown={onKey}>
    <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
    <div
      class="help"
      role="dialog"
      aria-label="Atajos de teclado"
      on:click|stopPropagation
      on:keydown={onKey}
    >
      <header>
        <h2>Atajos de teclado</h2>
        <button class="close" on:click={close} aria-label="Cerrar">×</button>
      </header>
      <div class="help-body">
        {#each groups as g}
          <section>
            <h3>{g.title}</h3>
            <dl>
              {#each g.rows as row}
                <dt>
                  {#each row.keys as k, i}
                    <kbd>{k}</kbd>{#if i < row.keys.length - 1}<span class="sep">después</span>{/if}
                  {/each}
                </dt>
                <dd>{row.label}</dd>
              {/each}
            </dl>
          </section>
        {/each}
        <section class="full">
          <h3>Saltos directos en la paleta (⌘K)</h3>
          <p class="muted" style="margin: 0 0 8px; font-size: 12px;">
            Escribe estas sintaxis dentro de la paleta para saltar directo,
            sin tener que navegar primero a la página.
          </p>
          <dl>
            {#each jumps as j}
              <dt><code>{j.syntax}</code></dt>
              <dd>{j.label}</dd>
            {/each}
          </dl>
        </section>
      </div>
    </div>
  </div>
{/if}

<style>
  .help-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    z-index: 300;
  }
  .help {
    width: min(720px, 100%);
    max-height: 80vh;
    overflow: auto;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 16px 64px rgba(0, 0, 0, 0.45);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 20px;
    border-bottom: 1px solid var(--border);
  }
  header h2 { margin: 0; font-size: 16px; }
  header .close {
    background: transparent;
    border: 0;
    font-size: 22px;
    line-height: 1;
    padding: 2px 8px;
    cursor: pointer;
  }
  .help-body {
    padding: 12px 20px 20px;
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 24px;
  }
  section h3 {
    margin: 8px 0 8px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
  }
  dl {
    margin: 0;
    display: grid;
    grid-template-columns: minmax(110px, max-content) 1fr;
    gap: 6px 14px;
    align-items: baseline;
  }
  dt {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-wrap: wrap;
  }
  dd { margin: 0; font-size: 13px; }
  kbd {
    font-family: "JetBrains Mono", Menlo, monospace;
    font-size: 11.5px;
    border: 1px solid var(--border);
    background: var(--bg);
    padding: 1px 6px;
    border-radius: 4px;
    min-width: 18px;
    text-align: center;
  }
  .sep {
    font-size: 10px;
    color: var(--text-muted);
    text-transform: lowercase;
  }
  section.full { grid-column: 1 / -1; }
  section.full code {
    background: var(--bg);
    border: 1px solid var(--border);
    padding: 1px 6px;
    border-radius: 4px;
    font-size: 11.5px;
  }
</style>
