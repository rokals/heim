<script>
  // Les scènes : une liste claire (active ou pas, jusqu'à quand), et pour chacune
  // activer, activer pour un temps, modifier, dupliquer. Un modèle pour démarrer vite.
  import Icon from './Icon.svelte';
  import Sheet from './Sheet.svelte';
  import SceneEditor from './SceneEditor.svelte';
  import { api, command, home, homeRooms, isAdmin, toast } from '../lib/store.svelte.js';
  import { sceneStatus, TEMPLATES } from '../lib/scenes.js';

  let details = $state([]);
  let editing = $state(null);
  let forScene = $state(null);
  let templates = $state(false);
  let tplRoom = $state('');

  async function load() {
    details = (await api('/scenes').catch(() => [])) ?? [];
  }

  // La liste suit les scènes déclarées (création, suppression, changement d'état).
  const sceneEntities = $derived(
    Object.values(home.entities)
      .filter((e) => e.domain === 'scene')
      .sort((a, b) => a.name.localeCompare(b.name, 'fr')),
  );
  // Une clé simple : on ne recharge que si une scène apparaît, disparaît ou change de nom.
  const sceneKey = $derived(sceneEntities.map((e) => e.entity_id + e.name).join('|'));
  $effect(() => {
    void sceneKey;
    load();
  });

  const byId = $derived(Object.fromEntries(details.map((d) => [d.id, d])));

  function membersText(d) {
    const ids = Object.keys(d?.targets ?? {});
    const names = ids.map((id) => home.entities[id]?.name).filter(Boolean);
    if (names.length <= 3) return names.join(', ');
    return `${names.slice(0, 3).join(', ')} et ${names.length - 3} autres`;
  }

  async function activateFor(id, minutes) {
    forScene = null;
    try {
      await api(`/scenes/${id}/activate`, { method: 'POST', body: { minutes } });
      toast(minutes >= 60 ? `Activée pour ${minutes / 60} h` : `Activée pour ${minutes} min`);
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  async function duplicate(id) {
    try {
      const copy = await api(`/scenes/${id}/duplicate`, { method: 'POST' });
      toast(`« ${copy.name} » créée`);
      await load();
      editing = { scene: copy };
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  async function remove(d) {
    if (!confirm(`Supprimer la scène « ${d.name} » ?`)) return;
    try {
      await api(`/scenes/${d.id}`, { method: 'DELETE' });
      editing = null;
      await load();
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  const DURATIONS = [15, 30, 60, 120, 240];
</script>

<header class="top">
  <h1>Scènes</h1>
  {#if isAdmin()}
    <div class="tools">
      <button class="btn quiet" onclick={() => (templates = true)}><Icon name="star" size={17} /> Modèles</button>
      <button class="btn" onclick={() => (editing = { scene: null })}><Icon name="plus" size={18} /> Nouvelle scène</button>
    </div>
  {/if}
</header>
<p class="intro">Une ambiance en un geste : chaque appareil réglé comme vous voulez (luminosité, blanc, couleur, effet). Si quelqu'un change une lampe ensuite, la scène se désactive d'elle-même.</p>

{#if !sceneEntities.length}
  <div class="empty">
    <p>Aucune scène pour l'instant.</p>
    {#if isAdmin()}<button class="btn" onclick={() => (templates = true)}>Partir d'un modèle</button>{/if}
  </div>
{/if}

<ul class="list">
  {#each sceneEntities as s (s.entity_id)}
    {@const id = s.entity_id.slice(6)}
    {@const d = byId[id]}
    {@const active = s.state?.value === 'on'}
    {@const tint = s.capabilities?.color}
    <li class:active style:--tint={tint || 'var(--ember)'}>
      <button class="toggle" aria-pressed={active} onclick={() => command(s.entity_id, active ? 'turn_off' : 'turn_on')}>
        <span class="ic"><Icon name={s.capabilities?.icon || 'scene'} size={22} /></span>
        <span class="text">
          <strong>{s.name}</strong>
          <span>{membersText(d)}</span>
          <span class="status">{sceneStatus(s, home.now)}{d?.off_mode === 'restore' ? ' · revient comme avant' : ''}</span>
        </span>
      </button>
      <div class="acts">
        <button class="mini" onclick={() => (forScene = d ?? { id, name: s.name })} title="Activer pour un temps" aria-label="Activer {s.name} pour un temps"><Icon name="timer" size={17} /></button>
        {#if isAdmin() && d}
          <button class="mini" onclick={() => (editing = { scene: d })} aria-label="Modifier {s.name}"><Icon name="pencil" size={16} /></button>
        {/if}
      </div>
    </li>
  {/each}
</ul>

{#if editing}
  <SceneEditor
    scene={editing.scene}
    template={editing.template ?? null}
    room={editing.room ?? ''}
    onclose={() => ((editing = null), load())}
    onduplicate={(id) => ((editing = null), duplicate(id))}
    ondelete={(d) => remove(d)}
  />
{/if}

{#if forScene}
  <Sheet title="« {forScene.name} » pendant…" onclose={() => (forScene = null)}>
    <p class="muted">Puis chaque appareil revient comme il était avant.</p>
    <div class="durations">
      {#each DURATIONS as m (m)}
        <button class="btn quiet" onclick={() => activateFor(forScene.id, m)}>{m < 60 ? `${m} min` : `${m / 60} h`}</button>
      {/each}
    </div>
  </Sheet>
{/if}

{#if templates}
  <Sheet title="Partir d'un modèle" onclose={() => (templates = false)}>
    <label class="field">
      <span>Pour la pièce</span>
      <select class="input" bind:value={tplRoom}>
        <option value="">Toute la maison</option>
        {#each homeRooms() as r (r)}<option value={r}>{r}</option>{/each}
      </select>
    </label>
    <div class="tpls">
      {#each TEMPLATES as t (t.id)}
        <button
          class="tpl"
          style:--tint={t.color}
          onclick={() => {
            templates = false;
            editing = { scene: null, template: { ...t, name: tplRoom ? `${t.name} ${tplRoom.toLowerCase()}` : t.name }, room: tplRoom };
          }}
        >
          <span class="ic"><Icon name={t.icon} size={22} /></span>
          <strong>{t.name}</strong>
          <span>{t.hint} · {t.pct} %</span>
        </button>
      {/each}
    </div>
    <p class="muted small">Le modèle prend les lumières de la pièce et propose des réglages ; vous ajustez ensuite chaque lampe.</p>
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

  .tools {
    display: flex;
    gap: 8px;
  }

  .intro {
    color: var(--muted);
    max-width: 60ch;
  }

  .empty {
    padding: 20px 0;
    color: var(--muted);
  }

  .list {
    list-style: none;
    margin: 18px 0 0;
    padding: 0;
    display: grid;
    gap: 10px;
  }

  .list li {
    display: flex;
    align-items: center;
    gap: 6px;
    padding-right: 8px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
    transition: background-color 0.3s ease;
  }

  .list li.active {
    background: color-mix(in srgb, var(--tint) 30%, var(--surface));
    box-shadow: var(--shadow), inset 0 0 0 1.5px color-mix(in srgb, var(--tint) 60%, transparent);
  }

  .toggle {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 14px;
    min-width: 0;
    padding: 14px 8px 14px 14px;
    text-align: left;
  }

  .ic {
    flex: none;
    display: grid;
    place-items: center;
    width: 46px;
    height: 46px;
    border-radius: 14px;
    background: var(--surface-2);
    color: var(--muted);
  }

  .active .ic {
    background: var(--tint);
    color: #fff;
  }

  .text {
    display: grid;
    min-width: 0;
  }

  .text strong {
    font-size: 18px;
  }

  .text span {
    font-size: 14px;
    color: var(--muted);
    overflow-wrap: anywhere;
  }

  .status {
    font-weight: 600;
  }

  .active .status {
    color: color-mix(in srgb, var(--tint) 65%, var(--ink));
  }

  .acts {
    display: flex;
    gap: 4px;
  }

  .mini {
    width: 40px;
    height: 40px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--muted);
  }

  .durations {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .tpls {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 10px;
  }

  .tpl {
    display: grid;
    gap: 6px;
    justify-items: start;
    padding: 14px;
    border-radius: 18px;
    background: color-mix(in srgb, var(--tint) 18%, var(--surface));
    box-shadow: var(--shadow);
    text-align: left;
  }

  .tpl .ic {
    background: var(--tint);
    color: #fff;
  }

  .tpl span {
    font-size: 13px;
    color: var(--muted);
  }

  .small {
    font-size: 13px;
  }
</style>
