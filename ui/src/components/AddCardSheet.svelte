<script>
  import { tr } from '../lib/i18n.js';
  // Ajouter un bouton à l'accueil : tout est là, rangé en trois onglets (Appareils, Scènes,
  // Automatismes). On touche une chose, puis on choisit ce que fera le bouton.
  import { untrack } from 'svelte';
  import Sheet from './Sheet.svelte';
  import Icon from './Icon.svelte';
  import { groupDevices } from '../lib/house.js';
  import { choicesFor, defaultCard, defaultIcon } from '../lib/tiles.js';
  import { iconFor } from '../lib/format.js';
  import { home, homeRooms } from '../lib/store.svelte.js';
  import { HOUSE_KINDS } from '../lib/housecards.js';
  import { isConfigured } from '../lib/security.js';

  let { dashboard, section = 0, onadd, onclose } = $props();

  let target = $state(untrack(() => section));
  let query = $state('');
  let tab = $state('devices');
  let open = $state(null);

  const used = $derived(new Set((dashboard?.sections ?? []).flatMap((s) => s.cards.map((c) => c.entity_id))));
  const all = $derived(Object.values(home.entities));
  const q = $derived(query.trim().toLowerCase());
  const match = (name, extra = '') => !q || `${name} ${extra}`.toLowerCase().includes(q);
  const byName = (a, b) => a.name.localeCompare(b.name, 'fr');

  const devices = $derived.by(() => {
    const out = [];
    for (const g of all.filter((e) => e.domain === 'group')) {
      out.push({ entity: g, name: g.name, where: tr("Groupe · {v}", { v: g.room || tr("Sans pièce") }), icon: 'bulb' });
    }
    for (const d of groupDevices(all).filter((d) => !d.hidden)) {
      if (d.controls.length) {
        for (const c of d.controls)
          out.push({ entity: c, name: d.controls.length > 1 ? c.name : d.name, where: d.room || tr("Sans pièce"), icon: iconFor(c) });
      } else if (d.readings.length) {
        // Un capteur à plusieurs mesures : chacune peut avoir son bouton.
        for (const r of d.readings)
          out.push({
            entity: r,
            name: d.readings.length > 1 ? `${d.name} · ${r.name.replace(d.name, '').trim() || r.name}` : d.name,
            where: d.room || tr("Sans pièce"),
            icon: iconFor(r),
          });
      }
    }
    for (const p of all.filter((e) => e.domain === 'person')) out.push({ entity: p, name: p.name, where: tr("Personne suivie"), icon: 'user' });
    return out.filter((m) => match(m.name, m.where)).sort((a, b) => a.where.localeCompare(b.where, 'fr') || byName(a, b));
  });
  const scenes = $derived(
    all
      .filter((e) => e.domain === 'scene' && !e.hidden)
      .map((e) => ({ entity: e, name: e.name, where: tr("Scène"), icon: 'scene' }))
      .filter((m) => match(m.name))
      .sort(byName),
  );
  const automations = $derived(
    all
      .filter((e) => e.domain === 'automation')
      .map((e) => ({
        entity: e,
        name: e.name,
        where: e.state?.attributes?.enabled === false ? tr("Désactivé") : e.state?.value === 'on' ? tr("État : vrai") : tr("État : faux"),
        icon: 'auto',
      }))
      .filter((m) => match(m.name))
      .sort(byName),
  );
  const HOUSE = $derived(
    [
      ...HOUSE_KINDS.map((k) => ({ id: k.kind, name: k.name, hint: k.hint, icon: k.icon, card: { type: 'house', kind: k.kind, w: k.w, h: k.h } })),
      ...OTHERS,
    ].filter((m) => match(m.name, m.hint)),
  );
  const alarmReady = $derived(isConfigured(home.security?.config));
  const OTHERS = [
    { id: 'title', name: tr("Titre"), hint: tr("Un intertitre pour regrouper des tuiles"), icon: 'pencil', card: { type: 'title', text: tr("Titre"), w: 4, h: 1 } },
    { id: 'spacer', name: tr("Espace vide"), hint: tr("Une case vide pour aérer ou aligner"), icon: 'plus', card: { type: 'spacer', w: 1, h: 1 } },
  ];
  const TABS = $derived([
    { id: 'devices', label: tr("Appareils"), items: devices },
    { id: 'scenes', label: tr("Scènes"), items: scenes },
    { id: 'automations', label: tr("Automatismes"), items: automations },
    { id: 'house', label: tr("Maison"), items: HOUSE },
  ]);
  const items = $derived(TABS.find((t) => t.id === tab).items);

  function add(m, choice) {
    onadd(target, defaultCard(m.entity, choice));
  }
</script>

<Sheet title={tr("Ajouter une tuile")} {onclose}>
  <div class="tabs" role="tablist">
    {#each TABS as t (t.id)}
      <button role="tab" aria-selected={tab === t.id} class:on={tab === t.id} onclick={() => ((tab = t.id), (open = null))}>
        {t.label}<span class="count">{t.items.length}</span>
      </button>
    {/each}
  </div>

  <div class="search">
    <Icon name="search" size={18} />
    <input class="input" type="search" placeholder={tr("Chercher par nom ou pièce")} bind:value={query} aria-label={tr("Chercher")} />
  </div>

  {#if dashboard?.sections?.length > 1}
    <label class="field">
      <span>{tr("Dans la section")}</span>
      <select class="input" bind:value={target}>
        {#each dashboard.sections as sec, k (k)}<option value={k}>{sec.title || tr("En haut (sans titre)")}</option>{/each}
      </select>
    </label>
  {/if}

  {#if tab === 'house'}
    <p class="muted small">{tr("Ces tuiles ne sont liées à aucun appareil : elles se recalculent toutes seules, une lampe ajoutée demain compte aussitôt.")}</p>
    <ul>
      {#each HOUSE as o (o.id)}
        {@const multi = o.id === 'room' || (o.id === 'leave' && alarmReady)}
        <li class:open={open === o.id}>
          <button class="row" onclick={() => (multi ? (open = open === o.id ? null : o.id) : onadd(target, structuredClone(o.card)))}>
            <span class="ic"><Icon name={o.icon} size={20} /></span>
            <span class="txt"><strong>{o.name}</strong><span>{o.hint}</span></span>
            <span class="chev" aria-hidden="true">{multi ? (open === o.id ? '–' : '›') : '+'}</span>
          </button>
          {#if open === o.id && o.id === 'room'}
            <div class="choices">
              {#each homeRooms() as r (r)}
                <button class="choice" onclick={() => onadd(target, { ...structuredClone(o.card), room: r })}><strong>{r}</strong><span>{tr("Lumières de la pièce")}</span></button>
              {:else}
                <p class="muted">{tr("Aucune pièce : créez-en dans Réglages, Maison.")}</p>
              {/each}
            </div>
          {:else if open === o.id && o.id === 'leave'}
            <div class="choices">
              <button class="choice" onclick={() => onadd(target, structuredClone(o.card))}><strong>{tr("Tout couper")}</strong><span>{tr("Lumières et prises")}</span></button>
              <button class="choice" onclick={() => onadd(target, { ...structuredClone(o.card), arm: true })}><strong>{tr("Et activer l’alarme")}</strong><span>{tr("Mode Absent")}</span></button>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {:else if !items.length}
    <p class="muted">
      {#if q}{tr("Rien ne correspond à « {query} ».", { query })}{:else if tab === 'automations'}{tr("Pas encore d'automatisme : créez-en un dans l'onglet Automatismes.")}{:else if tab === 'scenes'}{tr("Pas encore de scène.")}{:else}{tr("Aucun appareil.")}{/if}
    </p>
  {/if}

  <ul class:hidden={tab === 'house'}>
    {#each tab === 'house' ? [] : items as m (m.entity.entity_id)}
      {@const choices = choicesFor(m.entity)}
      <li class:open={open === m.entity.entity_id}>
        <button class="row" onclick={() => (choices.length === 1 ? add(m, choices[0]) : (open = open === m.entity.entity_id ? null : m.entity.entity_id))}>
          <span class="ic"><Icon name={m.icon || defaultIcon(m.entity)} size={20} /></span>
          <span class="txt">
            <strong>{m.name}</strong>
            <span>{m.where}{used.has(m.entity.entity_id) ? tr(" · déjà sur l’accueil") : ''}</span>
          </span>
          <span class="chev" aria-hidden="true">{choices.length === 1 ? '+' : open === m.entity.entity_id ? '–' : '›'}</span>
        </button>
        {#if open === m.entity.entity_id}
          <div class="choices">
            {#each choices as c (c.label)}
              <button class="choice" onclick={() => add(m, c)}>
                <strong>{c.label}</strong>
                <span>{c.hint}</span>
              </button>
            {/each}
          </div>
        {/if}
      </li>
    {/each}
  </ul>
</Sheet>

<style>
  .tabs {
    display: flex;
    padding: 3px;
    border-radius: 14px;
    background: var(--surface-2);
    margin-bottom: 12px;
  }

  .tabs button {
    flex: 1;
    min-height: 40px;
    border-radius: 11px;
    font-weight: 600;
    font-size: 15px;
    color: var(--muted);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
  }

  .tabs button.on {
    background: var(--surface);
    color: var(--ink);
    box-shadow: var(--shadow);
  }

  .count {
    font-size: 12px;
    font-weight: 600;
    padding: 0 6px;
    border-radius: 9px;
    background: color-mix(in srgb, currentColor 12%, transparent);
  }

  .search {
    position: relative;
    display: flex;
    align-items: center;
    color: var(--muted);
  }

  .search :global(svg) {
    position: absolute;
    left: 12px;
  }

  .search input {
    padding-left: 38px;
    width: 100%;
  }

  ul {
    list-style: none;
    padding: 0;
    margin: 12px 0 0;
  }

  li {
    border-bottom: 1px solid var(--line);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 10px 2px;
    text-align: left;
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

  .txt {
    flex: 1;
    display: grid;
    min-width: 0;
  }

  .txt span {
    font-size: 13px;
    color: var(--muted);
  }

  .chev {
    font-size: 22px;
    color: var(--muted);
    width: 24px;
    text-align: center;
  }

  .choices {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 8px;
    padding: 0 0 12px 52px;
  }

  .choice {
    display: grid;
    gap: 2px;
    padding: 10px 12px;
    border-radius: 14px;
    background: var(--surface);
    box-shadow: var(--shadow);
    text-align: left;
  }

  .choice span {
    font-size: 12px;
    color: var(--muted);
  }

  .field {
    margin-top: 10px;
  }

  .hidden {
    display: none;
  }

  .small {
    font-size: 13px;
    margin: 10px 0 0;
  }

  .tabs button {
    padding: 0 4px;
  }
</style>
