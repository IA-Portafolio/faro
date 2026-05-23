<script lang="ts">
  import { severityClass } from '$lib/stores';
  export let severity: string;
</script>

<span class="badge {severityClass(severity)}">{severity}</span>
