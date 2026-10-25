<script>
  // Qualité du signal Zigbee en barres, comme sur un téléphone. Le chiffre exact (LQI) est
  // dans l'infobulle et dans la fiche de l'appareil.
  import { signalBars } from '../lib/house.js';

  let { lqi } = $props();
  const bars = $derived(signalBars(lqi));
</script>

{#if bars != null}
  <span class="sig" class:weak={bars <= 1} title="Signal : {lqi} / 255" aria-label="Signal {bars} sur 4">
    {#each [1, 2, 3, 4] as b (b)}
      <span class="bar" class:on={b <= bars} style:height="{3 + b * 2.5}px"></span>
    {/each}
  </span>
{/if}

<style>
  .sig {
    display: inline-flex;
    align-items: flex-end;
    gap: 2px;
    height: 14px;
    color: var(--muted);
  }

  .bar {
    width: 3px;
    border-radius: 1px;
    background: currentColor;
    opacity: 0.25;
  }

  .bar.on {
    opacity: 1;
  }

  .weak {
    color: var(--alert);
  }
</style>
