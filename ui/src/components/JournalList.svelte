<script>
  import { tr } from '../lib/i18n.js';
  // Le journal : tout ce qui s'est passé, du plus récent au plus ancien, par jour.
  // Réutilisé partout : page Journal (avec filtres), fiche d'un appareil, d'un automatisme.
  import { untrack } from 'svelte';
  import { byDay, describe, when } from '../lib/journal.js';
  import { api, home } from '../lib/store.svelte.js';

  /** entity : une seule entité (ou plusieurs, séparées par des virgules côté appelant). */
  let { entity = null, entities = null, kind = '', q = '', limit = 50, compact = false } = $props();

  let rows = $state([]);
  let loading = $state(true);
  let more = $state(true);
  let error = $state('');

  function params(before) {
    const p = new URLSearchParams();
    if (entity) p.set('entity_id', entity);
    if (kind) p.set('kind', kind);
    if (q.trim()) p.set('q', q.trim());
    if (before) p.set('before', before);
    p.set('limit', String(limit));
    return p.toString();
  }

  async function load(reset = true) {
    loading = true;
    error = '';
    try {
      // Un appareil à plusieurs entités : on réunit leurs journaux.
      const ids = entities?.length ? entities : [entity];
      const before = reset ? null : rows.at(-1)?.id;
      const lists = await Promise.all(
        ids.map((id) => {
          const p = new URLSearchParams(params(before));
          if (id) p.set('entity_id', id);
          else p.delete('entity_id');
          return api(`/logbook?${p}`);
        }),
      );
      const merged = lists.flat().sort((a, b) => b.id - a.id).slice(0, limit);
      rows = reset ? merged : [...rows, ...merged];
      more = merged.length >= limit;
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  // Filtres changés : on recharge (avec un petit délai pour la recherche tapée).
  let timer;
  $effect(() => {
    void kind;
    void q;
    void entity;
    clearTimeout(timer);
    const first = untrack(() => rows.length === 0);
    timer = setTimeout(() => untrack(() => load()), first ? 0 : 250);
    return () => clearTimeout(timer);
  });

  const days = $derived(byDay(rows, new Date(home.now)));
</script>

{#if error}
  <p class="muted">{tr("Journal indisponible : {error}", { error })}</p>
{:else if !rows.length && !loading}
  <p class="muted">{tr("Rien pour l'instant.")}</p>
{/if}

<div class="journal" class:compact>
  {#each days as g (g.day)}
    {#if !compact}<h3>{g.day}</h3>{/if}
    <ul>
      {#each g.items as r (r.id)}
        {@const t = describe(r, home.entities)}
        <li class={t.tone}>
          <time datetime={r.ts}>{compact ? when(r.ts, new Date(home.now)) : when(r.ts, new Date(r.ts))}</time>
          <span class="txt">
            {#if !entity}<strong>{t.title}</strong>{/if}
            {t.text}
            {#if t.actor}<span class="by">{tr("par {actor}", { actor: t.actor })}</span>{/if}
          </span>
        </li>
      {/each}
    </ul>
  {/each}
</div>

{#if more && rows.length}
  <button class="btn quiet more" disabled={loading} onclick={() => load(false)}>{loading ? tr("Chargement…") : tr("Plus ancien")}</button>
{/if}

<style>
  h3 {
    font-size: 13px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    margin: 18px 0 6px;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  li {
    display: grid;
    grid-template-columns: 76px 1fr;
    gap: 10px;
    padding: 8px 0;
    border-bottom: 1px solid var(--line);
    font-size: 15px;
  }

  .compact li {
    grid-template-columns: 92px 1fr;
    font-size: 14px;
  }

  time {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    font-size: 14px;
  }

  .txt strong {
    margin-right: 4px;
  }

  .by {
    color: var(--muted);
    margin-left: 4px;
  }

  li.bad .txt {
    color: var(--alert);
  }

  li.muted .txt {
    color: var(--muted);
  }

  .more {
    margin-top: 10px;
  }
</style>
