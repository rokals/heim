<script>
  import { tr } from '../lib/i18n.js';
  // Tous les appareils sur une seule page : chercher, filtrer, et repérer d'un coup d'œil
  // les piles à changer et les appareils qui captent mal.
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import SignalBars from './SignalBars.svelte';
  import DeviceSheet from './DeviceSheet.svelte';
  import PairingSheet from './PairingSheet.svelte';
  import { ago, groupDevices, isAvailable, isOn, lastSeen, signal, UNSORTED } from '../lib/house.js';
  import { formatValue, iconFor } from '../lib/format.js';
  import { home, isAdmin, refreshEntities } from '../lib/store.svelte.js';

  const FILTERS = [
    { id: 'all', label: tr("Tous") },
    { id: 'controls', label: tr("Lumières et prises") },
    { id: 'sensors', label: tr("Capteurs") },
    { id: 'security', label: tr("Sirènes") },
    { id: 'unsorted', label: tr("Sans pièce") },
    { id: 'battery', label: tr("Piles") },
    { id: 'signal', label: tr("Signal") },
    { id: 'offline', label: tr("Injoignables") },
    { id: 'hidden', label: tr("Masqués") },
  ];

  let filter = $state('all');
  let query = $state('');
  let open = $state(null);
  let pairing = $state(location.hash.endsWith('/ajouter'));

  onMount(refreshEntities);

  const devices = $derived(
    groupDevices(Object.values(home.entities)).map((d) => ({
      ...d,
      lqi: signal(d),
      seen: lastSeen(d),
      offline: !isAvailable(d.primary),
    })),
  );

  const shown = $derived.by(() => {
    const q = query.trim().toLowerCase();
    let list = devices.filter((d) => (filter === 'hidden' ? d.hidden : !d.hidden));
    if (q) list = list.filter((d) => d.name.toLowerCase().includes(q) || (d.room || '').toLowerCase().includes(q));
    switch (filter) {
      case 'controls':
        return list.filter((d) => d.controls.length);
      case 'sensors':
        return list.filter((d) => !d.controls.length && !d.sirens.length);
      case 'security':
        return list.filter((d) => d.sirens.length);
      case 'unsorted':
        return list.filter((d) => d.room === UNSORTED);
      case 'battery':
        return list.filter((d) => d.battery != null).sort((a, b) => a.battery - b.battery);
      case 'signal':
        return list.filter((d) => d.lqi != null).sort((a, b) => a.lqi - b.lqi);
      case 'offline':
        return list.filter((d) => d.offline);
      default:
        return list;
    }
  });

  const counts = $derived({
    battery: devices.filter((d) => !d.hidden && d.battery != null && d.battery <= 20).length,
    offline: devices.filter((d) => !d.hidden && d.offline).length,
    unsorted: devices.filter((d) => !d.hidden && d.room === UNSORTED).length,
  });

  function mainValue(d) {
    const c = d.controls[0];
    if (c) return isOn(c) ? tr("Allumé") : tr("Éteint");
    if (d.sirens.length) return isOn(d.sirens[0]) ? tr("Sonne") : tr("Silencieuse");
    return d.readings[0] ? formatValue(d.readings[0]) : '–';
  }
</script>

<header class="top">
  <h1>{tr("Appareils")}</h1>
  <span class="count num">{devices.filter((d) => !d.hidden).length}</span>
  {#if 'zigbee2mqtt' in home.integrations && isAdmin()}
    <button class="btn add" onclick={() => (pairing = true)} aria-label={tr("Ajouter un appareil")}><Icon name="plus" size={18} /> {tr("Ajouter")}</button>
  {/if}
</header>

<input class="input search" type="search" bind:value={query} placeholder={tr("Chercher un appareil ou une pièce")} aria-label={tr("Chercher")} />

<div class="chips" role="group" aria-label={tr("Filtrer")}>
  {#each FILTERS.filter((f) => f.id !== 'security' || devices.some((d) => d.sirens.length)) as f (f.id)}
    <button class="chip" class:on={filter === f.id} aria-pressed={filter === f.id} onclick={() => (filter = f.id)}>
      {f.label}
      {#if f.id === 'battery' && counts.battery}<span class="dot num">{counts.battery}</span>{/if}
      {#if f.id === 'offline' && counts.offline}<span class="dot num">{counts.offline}</span>{/if}
      {#if f.id === 'unsorted' && counts.unsorted}<span class="dot num">{counts.unsorted}</span>{/if}
    </button>
  {/each}
</div>

{#if filter === 'battery'}
  <p class="note">{tr("Classés de la pile la plus faible à la plus pleine.")}</p>
{:else if filter === 'signal'}
  <p class="note">{tr("Classés du signal le plus faible au meilleur. Un appareil qui capte mal gagne souvent à avoir une prise Zigbee (qui relaie le signal) entre lui et la clé.")}</p>
{/if}

{#if !shown.length}
  <p class="note">{tr("Aucun appareil ici.")}</p>
{/if}

<ul class="list">
  {#each shown as d (d.key)}
    <li>
      <button class="row" class:offline={d.offline} onclick={() => (open = d)}>
        <span class="ic" class:lit={d.controls[0] && isOn(d.controls[0])}><Icon name={iconFor(d.primary)} size={20} /></span>
        <span class="txt">
          <strong>{d.name}</strong>
          <span>{d.room === UNSORTED ? tr("Sans pièce") : d.room}, {d.offline ? 'injoignable' : ago(d.seen, home.now)}</span>
        </span>
        <span class="meta">
          <span class="val num">{mainValue(d)}</span>
          <span class="tech">
            {#if d.battery != null}
              <span class="bat num" class:low={d.battery <= 20}><Icon name="battery" size={15} />{d.battery} %</span>
            {/if}
            <SignalBars lqi={d.lqi} />
          </span>
        </span>
      </button>
    </li>
  {/each}
</ul>

{#if pairing}
  <PairingSheet onclose={() => (pairing = false)} />
{/if}

{#if open}
  <DeviceSheet device={open} onclose={() => (open = null)} />
{/if}

<style>
  .top {
    display: flex;
    align-items: baseline;
    gap: 12px;
  }

  h1 {
    margin: 0;
    font-size: 40px;
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .add {
    margin-left: auto;
    align-self: center;
  }

  .count {
    font-size: 20px;
    color: var(--muted);
  }

  .search {
    margin: 16px 0 12px;
  }

  .chips {
    display: flex;
    gap: 8px;
    overflow-x: auto;
    padding-bottom: 6px;
    scrollbar-width: none;
  }

  .chip {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 40px;
    padding: 0 14px;
    border-radius: var(--radius-chip);
    border: 1.5px solid var(--line);
    font-weight: 600;
    font-size: 15px;
    white-space: nowrap;
  }

  .chip.on {
    background: var(--ink);
    border-color: var(--ink);
    color: var(--bg);
  }

  .dot {
    min-width: 20px;
    height: 20px;
    padding: 0 6px;
    border-radius: 10px;
    background: var(--alert);
    color: #fff;
    font-size: 12px;
    display: inline-grid;
    place-items: center;
  }

  .note {
    color: var(--muted);
    font-size: 15px;
    max-width: 60ch;
  }

  .list {
    list-style: none;
    margin: 8px 0 0;
    padding: 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 68px;
    padding: 8px 0;
    border-bottom: 1px solid var(--line);
    text-align: left;
  }

  .offline {
    opacity: 0.6;
  }

  .ic {
    flex: none;
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: 12px;
    background: var(--surface-2);
    color: var(--muted);
  }

  .ic.lit {
    background: var(--ember);
    color: var(--ember-ink);
  }

  .txt {
    display: grid;
    flex: 1;
    min-width: 0;
  }

  .txt strong,
  .txt span {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .txt span {
    font-size: 14px;
    color: var(--muted);
  }

  .meta {
    display: grid;
    justify-items: end;
    gap: 4px;
    flex: none;
  }

  .val {
    font-weight: 600;
  }

  .tech {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    color: var(--muted);
    font-size: 13px;
  }

  .bat {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }

  .bat.low {
    color: var(--alert);
    font-weight: 600;
  }
</style>
