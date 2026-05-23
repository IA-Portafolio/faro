<script lang="ts">
  import { timeRange, type RangePreset } from '$lib/stores';
  const presets: { v: RangePreset; label: string }[] = [
    { v: '5m', label: 'Últimos 5 min' },
    { v: '15m', label: 'Últimos 15 min' },
    { v: '1h', label: 'Última hora' },
    { v: '6h', label: 'Últimas 6 h' },
    { v: '24h', label: 'Últimas 24 h' },
    { v: '7d', label: 'Últimos 7 días' }
  ];
</script>

<div class="flex gap-8 center">
  <label class="muted" style="font-size: 12px;">Rango:</label>
  <select bind:value={$timeRange}>
    {#each presets as p}
      <option value={p.v}>{p.label}</option>
    {/each}
  </select>
</div>
