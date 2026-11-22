<script>
  import { tr } from '../lib/i18n.js';
  // Carte du réseau Zigbee : la clé, les routeurs et les appareils qui passent par eux.
  // Scannée chaque nuit à 3 h par la box ; « Relancer » refait le scan tout de suite.
  import Sheet from './Sheet.svelte';
  import Icon from './Icon.svelte';
  import { ago } from '../lib/house.js';
  import { api, home, isAdmin, toast } from '../lib/store.svelte.js';

  let { onclose } = $props();
  let data = $state(null);
  let closed = $state(new Set());

  async function load() {
    try {
      data = await api('/zigbee/map');
    } catch (e) {
      toast(e.message, 'alert');
    }
  }
  $effect(() => {
    home.zigbeeMapTick;
    load();
  });

  async function scan() {
    try {
      await api('/zigbee/map/scan', { method: 'POST' });
      await load();
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  const st = $derived(data?.status);
  const map = $derived(st?.map);
  const nodes = $derived(map?.nodes ?? []);
  const kids = $derived.by(() => {
    const m = new Map();
    for (const n of nodes) {
      if (!n.parent) continue;
      if (!m.has(n.parent)) m.set(n.parent, []);
      m.get(n.parent).push(n);
    }
    // Les routeurs d'abord, puis par nom.
    for (const list of m.values()) list.sort((a, b) => (a.role === 'router' ? 0 : 1) - (b.role === 'router' ? 0 : 1) || a.name.localeCompare(b.name, 'fr'));
    return m;
  });
  const roots = $derived(nodes.filter((n) => n.role === 'coordinator'));
  const lost = $derived(nodes.filter((n) => n.role !== 'coordinator' && !n.parent));
  const counts = $derived({
    routers: nodes.filter((n) => n.role === 'router').length,
    ends: nodes.filter((n) => n.role === 'end_device').length,
  });

  const tone = (q) => (q == null ? '' : q < 40 ? 'bad' : q < 90 ? 'mid' : 'good');
  const icon = (n) => (n.role === 'coordinator' ? 'chip' : n.role === 'router' ? 'plug' : 'sensor');
  const total = (id) => {
    let c = 0;
    for (const k of kids.get(id) ?? []) c += 1 + total(k.ieee);
    return c;
  };

  function toggle(id) {
    const s = new Set(closed);
    if (s.has(id)) s.delete(id);
    else s.add(id);
    closed = s;
  }
</script>

{#snippet row(n, depth)}
  {@const children = kids.get(n.ieee) ?? []}
  <li>
    <div class="node {n.role}" style:--depth={depth}>
      {#if children.length}
        <button class="fold" class:open={!closed.has(n.ieee)} onclick={() => toggle(n.ieee)} aria-label={closed.has(n.ieee) ? tr("Déplier") : tr("Replier")}>
          <Icon name="chevron" size={14} />
        </button>
      {:else}
        <span class="fold"></span>
      {/if}
      <span class="ico"><Icon name={icon(n)} size={16} /></span>
      <span class="what">
        <strong>{n.name}</strong>
        <small>
          {n.role === 'coordinator' ? tr("Clé (coordinateur)") : n.role === 'router' ? tr("Routeur") : tr("Appareil")}{data.rooms?.[n.ieee] ? ` · ${data.rooms[n.ieee]}` : ''}{children.length ? ` · ${tr("{n} appareil(s) derrière", { n: total(n.ieee) })}` : ''}
        </small>
      </span>
      {#if n.lqi != null}
        <span class="lqi {tone(n.lqi)}" title={tr("Qualité du lien (0-255)")}>
          <span class="bar"><span style:width="{Math.min(100, (n.lqi / 255) * 100)}%"></span></span>
          {n.lqi}
        </span>
      {/if}
    </div>
    {#if children.length && !closed.has(n.ieee)}
      <ul>
        {#each children as c (c.ieee)}{@render row(c, depth + 1)}{/each}
      </ul>
    {/if}
  </li>
{/snippet}

<Sheet title={tr("Carte du réseau Zigbee")} {onclose}>
  {#if !data}
    <p class="muted">{tr("Chargement…")}</p>
  {:else}
    <div class="head">
      <p class="muted">
        {#if map}
          {tr("Scan {when}, en {s} s : {r} routeur(s), {e} appareil(s).", { when: ago(map.scanned_at, home.now), s: map.duration_s, r: counts.routers, e: counts.ends })}
        {:else}
          {tr("Pas encore de carte.")}
        {/if}
        {tr("Mise à jour automatique chaque nuit à 3 h.")}
      </p>
      {#if isAdmin()}
        <button class="btn" onclick={scan} disabled={st.scanning}>
          <Icon name="restart" size={18} />
          {st.scanning ? tr("Scan en cours…") : map ? tr("Relancer le scan") : tr("Scanner maintenant")}
        </button>
      {/if}
    </div>
    {#if st.scanning}
      <p class="scanning"><span class="dot"></span>{tr("Zigbee2MQTT interroge chaque routeur : de quelques secondes à 2 minutes. Le réseau peut être un peu plus lent pendant ce temps.")}</p>
    {/if}
    {#if st.error}<p class="err">{st.error}</p>{/if}

    {#if map?.advice?.length || data.missing?.length}
      <details class="advice" open>
        <summary><Icon name="alert" size={16} /> {tr("Conseils ({n})", { n: (map?.advice?.length ?? 0) + (data.missing?.length ? 1 : 0) })}</summary>
        <ul>
          {#each map?.advice ?? [] as a, i (i)}<li class={a.kind}>{a.text}</li>{/each}
          {#if data.missing?.length}
            <li class="missing">{tr("Absents de la carte (éteints, piles vides ou endormis) : {list}", { list: data.missing.map((m) => m.name).join(', ') })}</li>
          {/if}
        </ul>
      </details>
    {/if}

    {#if map}
      <div class="legend">
        <span><i class="good"></i>{tr("bon")}</span>
        <span><i class="mid"></i>{tr("moyen")}</span>
        <span><i class="bad"></i>{tr("faible")}</span>
      </div>
      <ul class="tree">
        {#each roots as r (r.ieee)}{@render row(r, 0)}{/each}
      </ul>
      {#if lost.length}
        <h3>{tr("Sans chemin vers la clé pendant le scan")}</h3>
        <ul class="tree">
          {#each lost as n (n.ieee)}{@render row(n, 0)}{/each}
        </ul>
      {/if}
      <p class="muted small">{tr("Les appareils sur pile ne répondent que réveillés : ils peuvent apparaître sous leur dernier routeur connu.")}</p>
    {/if}
  {/if}
</Sheet>

<style>
  .head {
    display: grid;
    gap: 10px;
    margin-bottom: 10px;
  }

  .head p {
    margin: 0;
  }

  .head .btn {
    justify-self: start;
  }

  .scanning {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border-radius: 12px;
    background: var(--surface);
    font-size: 14px;
  }

  .dot {
    flex: none;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--ember);
    animation: pulse 1.2s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      opacity: 0.2;
    }
  }

  .err {
    color: var(--alert);
  }

  .advice {
    margin: 10px 0;
    padding: 10px 12px;
    border-radius: 12px;
    background: color-mix(in srgb, #d9a441 14%, var(--surface));
  }

  .advice summary {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
    cursor: pointer;
  }

  .advice ul {
    margin: 8px 0 0;
    padding-left: 18px;
    display: grid;
    gap: 6px;
    font-size: 14px;
  }

  .legend {
    display: flex;
    gap: 14px;
    margin: 12px 0 6px;
    font-size: 13px;
    color: var(--muted);
  }

  .legend i {
    display: inline-block;
    width: 10px;
    height: 10px;
    margin-right: 5px;
    border-radius: 50%;
  }

  .tree,
  .tree ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .tree ul {
    margin-left: 14px;
    padding-left: 8px;
    border-left: 1.5px solid var(--line);
  }

  .node {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    padding: 7px 8px 7px 2px;
    margin: 3px 0;
    border-radius: 12px;
    background: var(--surface);
  }

  .node.coordinator {
    background: var(--ink);
    color: var(--bg);
  }

  .node.coordinator small {
    color: inherit;
    opacity: 0.75;
  }

  .fold {
    flex: none;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    color: inherit;
    opacity: 0.7;
    transition: transform 0.15s ease;
  }

  .fold.open {
    transform: rotate(90deg);
  }

  .ico {
    flex: none;
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--ink);
  }

  .router .ico {
    background: color-mix(in srgb, var(--ember) 25%, var(--surface-2));
  }

  .what {
    flex: 1;
    min-width: 0;
    display: grid;
  }

  .what strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .what small {
    font-size: 12px;
    color: var(--muted);
  }

  .lqi {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }

  .bar {
    width: 36px;
    height: 6px;
    border-radius: 3px;
    background: var(--surface-2);
    overflow: hidden;
  }

  .bar span {
    display: block;
    height: 100%;
  }

  .good .bar span,
  i.good {
    background: var(--moss);
  }

  .mid .bar span,
  i.mid {
    background: #d9a441;
  }

  .bad .bar span,
  i.bad {
    background: var(--alert);
  }

  h3 {
    margin: 18px 0 6px;
    font-size: 15px;
  }

  .small {
    font-size: 13px;
    margin-top: 14px;
  }
</style>
