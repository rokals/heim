<script>
  import { tr } from '../lib/i18n.js';
  // La cloche en haut : tout ce que Heim a à dire (mises à jour, box, plugins, appareils,
  // alarme, automatismes, consommation). Le chiffre : les notes pas encore vues.
  import Icon from './Icon.svelte';
  import Sheet from './Sheet.svelte';
  import { ago } from '../lib/house.js';
  import { api, centerSeen, home, isAdmin, toast } from '../lib/store.svelte.js';

  let { onopen } = $props();
  let open = $state(false);
  let filter = $state('all');
  // Ce qui était non lu à l'ouverture : on le garde marqué pendant la lecture.
  let fresh = $state(null);

  const KINDS = {
    update: { icon: 'restart', label: tr("Mise à jour") },
    plugin: { icon: 'chip', label: tr("Plugin") },
    system: { icon: 'sensor', label: tr("Box") },
    device: { icon: 'battery', label: tr("Appareil") },
    automation: { icon: 'flow', label: tr("Automatisme") },
    security: { icon: 'shield', label: tr("Alarme") },
    presence: { icon: 'user', label: tr("Présence") },
    energy: { icon: 'bolt', label: tr("Consommation") },
    message: { icon: 'bell', label: tr("Message") },
  };
  const FILTERS = [
    { id: 'all', label: tr("Tout") },
    { id: 'active', label: tr("En cours") },
    { id: 'house', label: tr("Maison"), kinds: ['device', 'security', 'presence', 'automation', 'message'] },
    { id: 'box', label: tr("Box"), kinds: ['update', 'plugin', 'system'] },
    { id: 'energy', label: tr("Consommation"), kinds: ['energy'] },
  ];

  const notes = $derived(home.center?.notes ?? []);
  const unread = $derived(home.center?.unread ?? 0);
  const shown = $derived(
    notes.filter((n) => {
      const f = FILTERS.find((x) => x.id === filter);
      if (filter === 'active') return n.key && !n.resolved;
      return !f?.kinds || f.kinds.includes(n.kind);
    }),
  );
  const active = $derived(notes.filter((n) => n.key && !n.resolved));
  const urgent = $derived(active.some((n) => n.level === 'urgent'));

  function show() {
    const seen = home.center?.seen_at ? Date.parse(home.center.seen_at) : 0;
    fresh = new Set(notes.filter((n) => Date.parse(n.at) > seen).map((n) => n.id));
    open = true;
    centerSeen();
  }

  async function dismiss(n) {
    try {
      await api(`/center/${n.id}`, { method: 'DELETE' });
      home.center.notes = home.center.notes.filter((x) => x.id !== n.id);
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  async function clearAll() {
    try {
      await api('/center', { method: 'DELETE' });
      home.center.notes = home.center.notes.filter((x) => x.key && !x.resolved);
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  // « security » (ancien format) : la carte de l'alarme, dans Réglages > Sécurité.
  const target = (n) => (n.open === 'security' ? '#reglages/securite' : n.open);

  function go(n) {
    const t = target(n);
    if (!t?.startsWith('#')) return;
    open = false;
    onopen?.(t);
  }

  function day(at) {
    const d = new Date(at);
    const today = new Date();
    const y = new Date(Date.now() - 864e5);
    if (d.toDateString() === today.toDateString()) return tr("Aujourd'hui");
    if (d.toDateString() === y.toDateString()) return tr("Hier");
    return d.toLocaleDateString(undefined, { weekday: 'long', day: 'numeric', month: 'long' });
  }

  const groups = $derived.by(() => {
    const out = [];
    for (const n of shown) {
      const label = n.key && !n.resolved ? tr("En cours") : day(n.at);
      let g = out.find((x) => x.label === label);
      if (!g) out.push((g = { label, notes: [] }));
      g.notes.push(n);
    }
    // Les situations en cours d'abord.
    return out.sort((a, b) => (b.label === tr("En cours")) - (a.label === tr("En cours")));
  });
</script>

{#if home.center}
  <button
    class="bell"
    class:urgent
    onclick={show}
    aria-label={unread ? tr("Notifications : {n} nouvelle(s)", { n: unread }) : tr("Notifications")}
  >
    <Icon name="bell" size={22} />
    {#if unread}<span class="badge">{unread > 99 ? '99+' : unread}</span>{/if}
  </button>
{/if}

{#if open}
  <Sheet title={tr("Notifications")} onclose={() => (open = false)}>
    <div class="filters" role="tablist">
      {#each FILTERS as f (f.id)}
        <button role="tab" class="chip" class:on={filter === f.id} aria-selected={filter === f.id} onclick={() => (filter = f.id)}>
          {f.label}
          {#if f.id === 'active' && active.length}<b>{active.length}</b>{/if}
        </button>
      {/each}
    </div>

    {#if !shown.length}
      <p class="empty">
        <Icon name="check" size={28} />
        <span>{filter === 'active' ? tr("Rien en cours : tout va bien.") : tr("Aucune notification.")}</span>
      </p>
    {/if}

    {#each groups as g (g.label)}
      <h3>{g.label}</h3>
      <ul class="list">
        {#each g.notes as n (n.id)}
          {@const k = KINDS[n.kind] ?? KINDS.message}
          <li class="note {n.level}" class:resolved={n.resolved} class:fresh={fresh?.has(n.id)}>
            <span class="ico"><Icon name={k.icon} size={18} /></span>
            <button class="body" onclick={() => go(n)} disabled={!target(n)?.startsWith('#')}>
              <span class="top">
                <strong>{n.title ?? k.label}</strong>
                <small>{ago(n.at, home.now)}</small>
              </span>
              <span class="msg">{n.message}</span>
              {#if n.resolved}
                <span class="ok"><Icon name="check" size={14} /> {tr("Réglé")} · {ago(n.resolved, home.now)}{n.resolution ? ` · ${n.resolution}` : ''}</span>
              {:else if n.key}
                <span class="live">{tr("En cours")}{n.first !== n.at ? ` · ${tr("depuis")} ${ago(n.first, home.now)}` : ''}</span>
              {/if}
            </button>
            {#if !n.key || n.resolved}
              <button class="x" onclick={() => dismiss(n)} aria-label={tr("Retirer")}><Icon name="close" size={16} /></button>
            {/if}
          </li>
        {/each}
      </ul>
    {/each}

    {#if notes.some((n) => !n.key || n.resolved)}
      <div class="foot">
        <button class="btn quiet" onclick={clearAll}>{tr("Tout effacer")}</button>
        {#if isAdmin()}
          <button class="btn quiet" onclick={() => { open = false; onopen?.('#reglages/alertes'); }}>{tr("Régler les alertes")}</button>
        {/if}
      </div>
    {/if}
  </Sheet>
{/if}

<style>
  .bell {
    position: relative;
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: 50%;
    color: var(--ink);
  }

  .bell.urgent {
    color: var(--alert);
  }

  .badge {
    position: absolute;
    top: 4px;
    right: 2px;
    min-width: 18px;
    height: 18px;
    padding: 0 5px;
    border-radius: 9px;
    background: var(--alert);
    color: #fff;
    font-size: 11px;
    font-weight: 700;
    line-height: 18px;
    text-align: center;
  }

  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 8px;
  }

  .chip {
    min-height: 36px;
    padding: 0 14px;
    border-radius: 18px;
    border: 1px solid var(--line);
    font-weight: 600;
    font-size: 14px;
  }

  .chip.on {
    background: var(--ink);
    border-color: var(--ink);
    color: var(--bg);
  }

  .chip b {
    margin-left: 6px;
    padding: 0 6px;
    border-radius: 8px;
    background: var(--alert);
    color: #fff;
    font-size: 11px;
  }

  h3 {
    margin: 18px 0 8px;
    font-size: 14px;
    text-transform: capitalize;
    color: var(--muted);
  }

  .list {
    display: grid;
    gap: 8px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .note {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px;
    border-radius: 14px;
    background: var(--surface);
    border-left: 4px solid transparent;
  }

  .note.important {
    border-left-color: #d9a441;
  }

  .note.urgent {
    border-left-color: var(--alert);
  }

  .note.resolved {
    border-left-color: var(--moss);
    opacity: 0.8;
  }

  .note.fresh {
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--ember) 45%, transparent);
  }

  .ico {
    flex: none;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    background: var(--surface-2);
  }

  .urgent .ico {
    background: var(--alert);
    color: #fff;
  }

  .body {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 3px;
    text-align: left;
    color: inherit;
  }

  .body:disabled {
    cursor: default;
    opacity: 1;
  }

  .top {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }

  .top small,
  .ok,
  .live {
    font-size: 12px;
    color: var(--muted);
  }

  .msg {
    font-size: 14px;
    overflow-wrap: anywhere;
  }

  .ok {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--moss);
  }

  .live {
    color: var(--alert);
    font-weight: 600;
  }

  .x {
    flex: none;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    color: var(--muted);
  }

  .empty {
    display: grid;
    justify-items: center;
    gap: 8px;
    padding: 30px 0;
    color: var(--muted);
  }

  .foot {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 16px;
  }
</style>
