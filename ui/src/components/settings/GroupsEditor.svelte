<script>
  // Groupes : plusieurs lumières ou prises commandées comme une seule (« Tout le salon »).
  import { onMount } from 'svelte';
  import Icon from '../Icon.svelte';
  import { api, home, homeRooms, toast } from '../../lib/store.svelte.js';

  let groups = $state([]);
  let editing = $state(null);
  let filter = $state('');

  onMount(async () => {
    groups = (await api('/groups').catch(() => [])) ?? [];
  });

  const candidates = $derived(
    Object.values(home.entities)
      .filter((e) => (e.domain === 'light' || e.domain === 'switch') && !e.hidden && e.category !== 'diagnostic')
      .sort((a, b) => (a.room || '~').localeCompare(b.room || '~', 'fr') || a.name.localeCompare(b.name, 'fr')),
  );
  const shown = $derived(filter ? candidates.filter((e) => (e.room || '') === filter) : candidates);
  const nameOf = (id) => home.entities[id]?.name ?? id;

  async function save(next) {
    try {
      groups = await api('/groups', { method: 'PUT', body: next });
      return true;
    } catch (e) {
      toast(e.message, 'alert');
      return false;
    }
  }

  function start(g = null) {
    editing = g ? { ...structuredClone($state.snapshot(g)), index: groups.indexOf(g) } : { id: '', name: '', room: '', members: [], index: -1 };
    filter = editing.room || '';
  }

  function toggle(id) {
    const m = editing.members;
    const i = m.indexOf(id);
    if (i >= 0) m.splice(i, 1);
    else m.push(id);
  }

  async function commit() {
    const { index, ...g } = $state.snapshot(editing);
    const next = $state.snapshot(groups);
    if (index >= 0) next[index] = g;
    else next.push(g);
    if (await save(next)) {
      toast(index >= 0 ? 'Groupe enregistré' : 'Groupe créé : il apparaît dans sa pièce');
      editing = null;
    }
  }

  async function remove(g) {
    if (!confirm(`Supprimer le groupe « ${g.name} » ? Les appareils, eux, restent.`)) return;
    await save($state.snapshot(groups).filter((x) => x.id !== g.id));
  }
</script>

<p class="muted">Plusieurs lumières ou prises commandées d'un seul bouton. Un groupe a sa carte sur l'accueil et s'utilise dans les automatismes.</p>

{#if groups.length}
  <ul class="list">
    {#each groups as g (g.id)}
      <li>
        <span class="ic"><Icon name="bulb" size={20} /></span>
        <span class="txt">
          <strong>{g.name}</strong>
          <span>{g.room || 'Sans pièce'} · {g.members.map(nameOf).join(', ')}</span>
        </span>
        <button class="mini" onclick={() => start(g)} aria-label="Modifier {g.name}"><Icon name="pencil" size={15} /></button>
        <button class="mini" onclick={() => remove(g)} aria-label="Supprimer {g.name}"><Icon name="close" size={15} /></button>
      </li>
    {/each}
  </ul>
{/if}

{#if editing}
  <div class="editor">
    <label class="field">
      <span>Nom du groupe</span>
      <input class="input" bind:value={editing.name} placeholder="Tout le salon" />
    </label>
    <label class="field">
      <span>Pièce</span>
      <select class="input" bind:value={editing.room} onchange={() => (filter = editing.room)}>
        <option value="">Sans pièce</option>
        {#each homeRooms() as r (r)}<option value={r}>{r}</option>{/each}
      </select>
    </label>
    <div class="field">
      <span>Appareils ({editing.members.length} choisis, deux au moins)</span>
      <div class="chips">
        <button class="chip" class:on={filter === ''} onclick={() => (filter = '')}>Toutes les pièces</button>
        {#if editing.room}<button class="chip" class:on={filter === editing.room} onclick={() => (filter = editing.room)}>{editing.room}</button>{/if}
      </div>
      <ul class="pick">
        {#each shown as e (e.entity_id)}
          <li>
            <label>
              <input type="checkbox" checked={editing.members.includes(e.entity_id)} onchange={() => toggle(e.entity_id)} />
              <Icon name={e.domain === 'light' ? 'bulb' : 'plug'} size={16} />
              <span>{e.name}</span>
              <small>{e.room || 'Sans pièce'}</small>
            </label>
          </li>
        {/each}
      </ul>
    </div>
    <div class="row">
      <button class="btn" onclick={commit} disabled={!editing.name.trim() || editing.members.length < 2}>Enregistrer</button>
      <button class="btn quiet" onclick={() => (editing = null)}>Annuler</button>
    </div>
  </div>
{:else}
  <button class="btn quiet" onclick={() => start()}><Icon name="plus" size={16} /> Nouveau groupe</button>
{/if}

<style>
  .list,
  .pick {
    list-style: none;
    margin: 10px 0;
    padding: 0;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
    overflow: hidden;
  }

  .list li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
  }

  .list li + li,
  .pick li + li {
    border-top: 1px solid var(--line);
  }

  .ic {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: 12px;
    background: var(--surface-2);
    flex: none;
  }

  .txt {
    flex: 1;
    display: grid;
    min-width: 0;
  }

  .txt span {
    font-size: 13px;
    color: var(--muted);
    overflow-wrap: anywhere;
  }

  .mini {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--muted);
    flex: none;
  }

  .editor {
    padding: 14px;
    border-radius: var(--radius-tile);
    background: color-mix(in srgb, var(--surface-2) 60%, transparent);
    border: 1px solid var(--line);
  }

  .pick {
    max-height: 320px;
    overflow-y: auto;
  }

  .pick label {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
  }

  .pick input {
    width: 20px;
    height: 20px;
    accent-color: var(--ink);
  }

  .pick span {
    flex: 1;
  }

  .pick small {
    color: var(--muted);
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chip {
    padding: 6px 12px;
    border-radius: var(--radius-chip);
    background: var(--surface-2);
    font-weight: 600;
    font-size: 13px;
  }

  .chip.on {
    background: var(--ink);
    color: var(--bg);
  }

  .row {
    display: flex;
    gap: 8px;
  }
</style>
