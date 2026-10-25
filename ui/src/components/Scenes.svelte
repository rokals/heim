<script>
  // Une scène se crée en photographiant l'éclairage tel qu'il est : on règle les lampes
  // comme on les aime, puis « Enregistrer cette ambiance ».
  import Icon from './Icon.svelte';
  import Sheet from './Sheet.svelte';
  import { api, command, home, isAdmin, toast } from '../lib/store.svelte.js';
  import { isAvailable, isOn, slug } from '../lib/house.js';

  const all = $derived(Object.values(home.entities));
  const scenes = $derived(all.filter((e) => e.domain === 'scene').sort((a, b) => a.name.localeCompare(b.name, 'fr')));
  const controls = $derived(
    all
      .filter((e) => (e.domain === 'light' || e.domain === 'switch') && !e.hidden && isAvailable(e))
      .sort((a, b) => (a.room || '~').localeCompare(b.room || '~', 'fr') || a.name.localeCompare(b.name, 'fr')),
  );

  let creating = $state(false);
  let name = $state('');
  let picked = $state({});
  let busy = $state(false);

  function startCreate() {
    name = '';
    // Par défaut : les lumières allumées en ce moment.
    picked = Object.fromEntries(controls.map((e) => [e.entity_id, e.domain === 'light' && isOn(e)]));
    creating = true;
  }

  async function create(e) {
    e.preventDefault();
    const entities = Object.keys(picked).filter((id) => picked[id]);
    if (!name.trim() || !entities.length) return;
    busy = true;
    try {
      await api(`/scenes/${slug(name)}/capture`, { method: 'POST', body: { name: name.trim(), entities } });
      toast(`Scène « ${name.trim()} » enregistrée`);
      creating = false;
    } catch (err) {
      toast(err.message, 'alert');
    } finally {
      busy = false;
    }
  }

  async function remove(scene) {
    if (!confirm(`Supprimer la scène « ${scene.name} » ?`)) return;
    try {
      await api(`/scenes/${scene.entity_id.slice(6)}`, { method: 'DELETE' });
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  function describe(scene) {
    const members = scene.capabilities?.members || [];
    const names = members.map((id) => home.entities[id]?.name).filter(Boolean);
    if (names.length <= 3) return names.join(', ');
    return `${names.slice(0, 3).join(', ')} et ${names.length - 3} autres`;
  }
</script>

<header class="top">
  <h1>Scènes</h1>
  {#if isAdmin()}<button class="btn" onclick={startCreate}><Icon name="plus" size={18} /> Enregistrer cette ambiance</button>{/if}
</header>
<p class="intro">Réglez vos lumières comme vous les aimez, puis enregistrez l'ambiance. Si quelqu'un change une lampe ensuite, la scène se désactive d'elle-même.</p>

{#if !scenes.length}
  <p class="empty">Aucune scène pour l'instant.</p>
{/if}

<ul class="list">
  {#each scenes as s (s.entity_id)}
    {@const active = isOn(s)}
    <li class:active>
      <button class="toggle" aria-pressed={active} onclick={() => command(s.entity_id, active ? 'turn_off' : 'turn_on')}>
        <span class="dot" aria-hidden="true"></span>
        <span class="text">
          <strong>{s.name}</strong>
          <span>{describe(s)}</span>
          <span class="status">{active ? (s.state?.attributes?.partial ? 'Active, un appareil injoignable' : 'Active') : 'Inactive'}</span>
        </span>
      </button>
      {#if isAdmin()}<button class="del" onclick={() => remove(s)} aria-label="Supprimer {s.name}"><Icon name="close" size={18} /></button>{/if}
    </li>
  {/each}
</ul>

{#if creating}
  <Sheet title="Nouvelle scène" onclose={() => (creating = false)}>
    <form onsubmit={create}>
      <label class="field">
        <span>Nom</span>
        <input class="input" bind:value={name} placeholder="Soirée film, Dîner, Réveil…" autocomplete="off" />
      </label>
      <fieldset>
        <legend>Appareils à inclure, dans leur état actuel</legend>
        {#each controls as e (e.entity_id)}
          <label class="pick">
            <input type="checkbox" bind:checked={picked[e.entity_id]} />
            <span>{e.name}</span>
            <span class="state">{e.room ?? ''} {isOn(e) ? 'allumé' : 'éteint'}</span>
          </label>
        {/each}
      </fieldset>
      <button class="btn" type="submit" disabled={busy || !name.trim()}>Enregistrer la scène</button>
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

  .intro {
    color: var(--muted);
    max-width: 60ch;
  }

  .empty {
    color: var(--muted);
  }

  .list {
    list-style: none;
    padding: 0;
    margin: 20px 0 0;
    display: grid;
    gap: 8px;
  }

  li {
    display: flex;
    align-items: center;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .toggle {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 16px;
    text-align: left;
    min-width: 0;
  }

  .dot {
    flex: none;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2px solid var(--muted);
  }

  .active .dot {
    background: var(--ember);
    border-color: var(--ember);
    box-shadow: 0 0 0 5px color-mix(in srgb, var(--ember) 25%, transparent);
  }

  .text {
    display: grid;
    min-width: 0;
  }

  .text span {
    font-size: 14px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .active .status {
    color: color-mix(in srgb, var(--ember) 70%, var(--ink));
    font-weight: 600;
  }

  .del {
    width: 48px;
    height: 48px;
    display: grid;
    place-items: center;
    color: var(--muted);
  }

  fieldset {
    border: 0;
    padding: 0;
    margin: 0 0 20px;
  }

  legend {
    font-size: 14px;
    color: var(--muted);
    margin-bottom: 6px;
  }

  .pick {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 48px;
    border-bottom: 1px solid var(--line);
  }

  .pick input {
    width: 20px;
    height: 20px;
    accent-color: var(--ink);
  }

  .pick .state {
    margin-left: auto;
    font-size: 14px;
    color: var(--muted);
  }
</style>
