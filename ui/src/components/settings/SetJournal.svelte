<script>
  import { tr } from '../../lib/i18n.js';
  // Le journal de toute la maison : qui a allumé quoi, quand la porte s'est ouverte,
  // pourquoi un automatisme ne s'est pas lancé, les coupures de Zigbee2MQTT…
  import Icon from '../Icon.svelte';
  import JournalList from '../JournalList.svelte';
  import { KINDS } from '../../lib/journal.js';

  let kind = $state('');
  let q = $state('');
</script>

<p class="muted">{tr("Tout ce qui s'est passé ces 30 derniers jours, le plus récent en haut.")}</p>

<div class="filters">
  <div class="chips" role="radiogroup" aria-label={tr("Filtrer")}>
    <button class="chip" class:on={kind === ''} onclick={() => (kind = '')}>{tr("Tout")}</button>
    {#each KINDS as k (k.id)}
      <button class="chip" class:on={kind === k.id} onclick={() => (kind = k.id)}>{k.label}</button>
    {/each}
  </div>
  <div class="search">
    <Icon name="search" size={18} />
    <input class="input" type="search" bind:value={q} placeholder={tr("Chercher : salon, Alex, courrier…")} aria-label={tr("Chercher dans le journal")} />
  </div>
</div>

<JournalList {kind} {q} limit={80} />

<style>
  .filters {
    display: grid;
    gap: 10px;
    margin: 12px 0 4px;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chip {
    padding: 7px 14px;
    border-radius: var(--radius-chip);
    background: var(--surface-2);
    font-weight: 600;
    font-size: 14px;
  }

  .chip.on {
    background: var(--ink);
    color: var(--bg);
  }

  .search {
    position: relative;
    display: flex;
    align-items: center;
    color: var(--muted);
    max-width: 460px;
  }

  .search :global(svg) {
    position: absolute;
    left: 12px;
  }

  .search input {
    width: 100%;
    padding-left: 38px;
  }
</style>
