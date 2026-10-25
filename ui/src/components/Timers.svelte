<script>
  import Icon from './Icon.svelte';
  import Sheet from './Sheet.svelte';
  import TimerTile from './TimerTile.svelte';
  import { api, home, toast } from '../lib/store.svelte.js';
  import { slug } from '../lib/house.js';

  const all = $derived(Object.values(home.entities));
  const timers = $derived(all.filter((e) => e.domain === 'timer').sort((a, b) => a.name.localeCompare(b.name, 'fr')));
  const controls = $derived(
    all.filter((e) => (e.domain === 'light' || e.domain === 'switch') && !e.hidden).sort((a, b) => a.name.localeCompare(b.name, 'fr')),
  );

  const PRESETS = [5, 10, 15, 30, 45, 60, 90];

  let creating = $state(false);
  let name = $state('');
  let minutes = $state(30);
  let then = $state('');
  let startNow = $state(true);
  let busy = $state(false);

  function open() {
    name = '';
    minutes = 30;
    then = '';
    startNow = true;
    creating = true;
  }

  async function create(e) {
    e.preventDefault();
    const id = slug(name);
    if (!id || !(minutes > 0)) return;
    busy = true;
    try {
      const body = { name: name.trim(), duration_s: Math.round(minutes * 60) };
      if (then) body.on_finish = { entity_id: then, action: 'turn_off' };
      const room = then ? home.entities[then]?.room : undefined;
      if (room) body.room = room;
      await api(`/timers/${id}`, { method: 'PUT', body });
      if (startNow) await api(`/timers/${id}/control`, { method: 'POST', body: { op: 'start' } });
      creating = false;
    } catch (err) {
      toast(err.message, 'alert');
    } finally {
      busy = false;
    }
  }

  async function remove(t) {
    if (!confirm(`Supprimer le minuteur « ${t.name} » ?`)) return;
    try {
      await api(`/timers/${t.entity_id.slice(6)}`, { method: 'DELETE' });
    } catch (err) {
      toast(err.message, 'alert');
    }
  }
</script>

<header class="top">
  <h1>Minuteurs</h1>
  <button class="btn" onclick={open}><Icon name="plus" size={18} /> Nouveau minuteur</button>
</header>
<p class="intro">Un minuteur peut éteindre un appareil tout seul à la fin : la cafetière après 30 minutes, la chaufferette après une heure. Il continue même si Heim redémarre.</p>

{#if !timers.length}
  <p class="empty">Aucun minuteur pour l'instant.</p>
{/if}

<div class="list">
  {#each timers as t (t.entity_id)}
    <div class="item">
      <TimerTile entity={t} />
      <button class="del" onclick={() => remove(t)} aria-label="Supprimer {t.name}">Supprimer</button>
    </div>
  {/each}
</div>

{#if creating}
  <Sheet title="Nouveau minuteur" onclose={() => (creating = false)}>
    <form onsubmit={create}>
      <label class="field">
        <span>Nom</span>
        <input class="input" bind:value={name} placeholder="Cafetière, Lessive, Four…" autocomplete="off" />
      </label>

      <div class="field">
        <span>Durée</span>
        <div class="presets" role="group" aria-label="Durées courantes">
          {#each PRESETS as p (p)}
            <button type="button" class="preset" class:on={minutes === p} aria-pressed={minutes === p} onclick={() => (minutes = p)}>
              {p < 60 ? `${p} min` : p === 60 ? '1 h' : '1 h 30'}
            </button>
          {/each}
        </div>
        <label class="custom">
          <input class="input num" type="number" min="1" max="10080" bind:value={minutes} />
          <span>minutes</span>
        </label>
      </div>

      <label class="field">
        <span>À la fin, éteindre</span>
        <select class="input" bind:value={then}>
          <option value="">Rien, juste prévenir</option>
          {#each controls as c (c.entity_id)}
            <option value={c.entity_id}>{c.name}{c.room ? ` (${c.room})` : ''}</option>
          {/each}
        </select>
      </label>

      <label class="check">
        <input type="checkbox" bind:checked={startNow} />
        Démarrer tout de suite
      </label>

      <button class="btn" type="submit" disabled={busy || !name.trim()}>Créer le minuteur</button>
    </form>
  </Sheet>
{/if}

<style>
  .top {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  h1 {
    margin: 0;
    font-size: 40px;
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .intro,
  .empty {
    color: var(--muted);
    max-width: 60ch;
  }

  .list {
    display: grid;
    gap: 14px;
    margin-top: 20px;
  }

  .item {
    display: grid;
    gap: 2px;
  }

  .del {
    justify-self: end;
    font-size: 14px;
    color: var(--muted);
    padding: 6px 4px;
  }

  .presets {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .preset {
    min-height: 40px;
    padding: 0 14px;
    border-radius: var(--radius-chip);
    border: 1.5px solid var(--line);
    font-weight: 600;
  }

  .preset.on {
    background: var(--ink);
    border-color: var(--ink);
    color: var(--bg);
  }

  .custom {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 8px;
    color: var(--muted);
  }

  .custom .input {
    width: 110px;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 20px;
  }

  .check input {
    width: 20px;
    height: 20px;
    accent-color: var(--ink);
  }
</style>
