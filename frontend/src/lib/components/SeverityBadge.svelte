<script lang="ts">
  /**
   * Etiqueta (badge) de color para el nivel de severidad de un log o evento.
   *
   * Recibe `severity` (TRACE/DEBUG/INFO/WARN/ERROR/FATAL) y le aplica la clase CSS
   * que devuelve `severityClass()`, para colorearla igual en toda la app.
   */
  import { severityClass } from '$lib/stores';
  export let severity: string;
</script>

<span class="badge {severityClass(severity)}">{severity}</span>
