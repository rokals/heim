<script>
  // La maison : nom, étages, pièces (dans l'ordre voulu), et les appareils à ranger.
  import Icon from '../Icon.svelte';
  import GroupsEditor from './GroupsEditor.svelte';
  import { api, home, homeRooms, saveLayout, toast, updateDevice } from '../../lib/store.svelte.js';
  import { groupDevices, UNSORTED } from '../../lib/house.js';
  import { FLOOR_PRESETS, ROOM_ICONS, ROOM_PRESETS, roomIcon } from '../../lib/rooms.js';

  const layout = $derived(home.layout ?? { name: '', floors: [], rooms: [] });
  const devices = $derived(groupDevices(Object.values(home.entities)).filter((d) => !d.hidden));
  const countIn = (room) => devices.filter((d) => d.room === room).length;
  const unsorted = $derived(devices.filter((d) => d.room === UNSORTED));
  const strays = $derived(homeRooms().filter((r) => !layout.rooms.some((x) => x.name === r)));
  const presets = $derived(ROOM_PRESETS.filter((p) => !layout.rooms.some((r) => r.name.toLowerCase() === p.name.toLowerCase())));

  let newRoom = $state('');
  let newFloor = $state('');
  let iconFor = $state(null);
  let editing = $state(null);

  async function save(next) {
    try {
      await saveLayout(next);
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  const copy = () => structuredClone($state.snapshot(layout));

  function addRoom(name, icon = null) {
    name = name.trim();
    if (!name) return;
    if (layout.rooms.some((r) => r.name.toLowerCase() === name.toLowerCase())) return toast('Cette pièce existe déjà', 'alert');
    const l = copy();
    l.rooms.push({ name, icon: icon ?? undefined, floor: l.floors.length === 1 ? l.floors[0] : undefined });
    save(l);
    newRoom = '';
  }

  function move(i, delta) {
    const l = copy();
    const j = i + delta;
    if (j < 0 || j >= l.rooms.length) return;
    [l.rooms[i], l.rooms[j]] = [l.rooms[j], l.rooms[i]];
    save(l);
  }

  async function rename(i, to) {
    to = to.trim();
    const from = layout.rooms[i].name;
    editing = null;
    if (!to || to === from) return;
    try {
      if (countIn(from)) await api('/rooms/rename', { method: 'POST', body: { from, to } });
      const l = copy();
      l.rooms[i].name = to;
      await save(l);
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  async function remove(i) {
    const r = layout.rooms[i];
    const n = countIn(r.name);
    if (!confirm(n ? `Supprimer la pièce « ${r.name} » ? Ses ${n} appareil(s) passeront dans « Sans pièce ».` : `Supprimer la pièce « ${r.name} » ?`)) return;
    try {
      if (n) await api('/rooms/rename', { method: 'POST', body: { from: r.name, to: null } });
      const l = copy();
      l.rooms.splice(i, 1);
      await save(l);
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  function setIcon(i, icon) {
    const l = copy();
    l.rooms[i].icon = icon;
    save(l);
    iconFor = null;
  }

  function setFloor(i, floor) {
    const l = copy();
    l.rooms[i].floor = floor || undefined;
    save(l);
  }

  function addFloor(name) {
    name = name.trim();
    if (!name || layout.floors.includes(name)) return;
    const l = copy();
    l.floors.push(name);
    save(l);
    newFloor = '';
  }

  function removeFloor(f) {
    const l = copy();
    l.floors = l.floors.filter((x) => x !== f);
    l.rooms.forEach((r) => r.floor === f && delete r.floor);
    save(l);
  }

  async function place(d, room) {
    try {
      await updateDevice(d, { room });
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  const byFloor = $derived.by(() => {
    if (!layout.floors.length) return [{ floor: null, rooms: layout.rooms.map((r, i) => ({ r, i })) }];
    const groups = [...layout.floors, null].map((floor) => ({
      floor,
      rooms: layout.rooms.map((r, i) => ({ r, i })).filter(({ r }) => (r.floor ?? null) === floor),
    }));
    return groups.filter((g) => g.floor || g.rooms.length);
  });
</script>

<label class="field">
  <span>Nom de la maison</span>
  <input
    class="input"
    value={layout.name}
    placeholder="La maison, Chez nous…"
    onchange={(e) => save({ ...copy(), name: e.currentTarget.value })}
  />
</label>

<h2>Étages</h2>
<p class="muted">Facultatif : pour une maison de plain-pied, n'en créez aucun.</p>
<div class="chips">
  {#each layout.floors as f (f)}
    <span class="chip on">{f}<button aria-label="Retirer {f}" onclick={() => removeFloor(f)}><Icon name="close" size={14} /></button></span>
  {/each}
  {#each FLOOR_PRESETS.filter((f) => !layout.floors.includes(f)) as f (f)}
    <button class="chip" onclick={() => addFloor(f)}><Icon name="plus" size={14} /> {f}</button>
  {/each}
</div>
<form
  class="add"
  onsubmit={(e) => {
    e.preventDefault();
    addFloor(newFloor);
  }}
>
  <input class="input" bind:value={newFloor} placeholder="Autre étage" />
  <button class="btn quiet" type="submit" disabled={!newFloor.trim()}>Ajouter</button>
</form>

<h2>Pièces</h2>
{#if !layout.rooms.length}
  <p class="muted">Ajoutez vos pièces : elles serviront à ranger les appareils et l'accueil suivra cet ordre.</p>
{/if}

{#each byFloor as g (g.floor ?? '—')}
  {#if layout.floors.length}<h3>{g.floor ?? 'Sans étage'}</h3>{/if}
  <ul class="rooms">
    {#each g.rooms as { r, i } (r.name)}
      <li>
        <button class="ic" aria-label="Icône de {r.name}" onclick={() => (iconFor = iconFor === i ? null : i)}><Icon name={roomIcon(r)} size={22} /></button>
        <div class="txt">
          {#if editing === i}
            <input class="input" value={r.name} onblur={(e) => rename(i, e.currentTarget.value)} onkeydown={(e) => e.key === 'Enter' && e.currentTarget.blur()} />
          {:else}
            <button class="name" onclick={() => (editing = i)}>{r.name}</button>
          {/if}
          <span>{countIn(r.name) ? `${countIn(r.name)} appareil(s)` : 'vide'}</span>
        </div>
        {#if layout.floors.length}
          <select class="input floor" value={r.floor ?? ''} onchange={(e) => setFloor(i, e.currentTarget.value)} aria-label="Étage de {r.name}">
            <option value="">—</option>
            {#each layout.floors as f (f)}<option value={f}>{f}</option>{/each}
          </select>
        {/if}
        <button class="mini" aria-label="Monter {r.name}" disabled={i === 0} onclick={() => move(i, -1)}>↑</button>
        <button class="mini" aria-label="Descendre {r.name}" disabled={i === layout.rooms.length - 1} onclick={() => move(i, 1)}>↓</button>
        <button class="mini" aria-label="Supprimer {r.name}" onclick={() => remove(i)}><Icon name="close" size={15} /></button>
      </li>
      {#if iconFor === i}
        <li class="icons">
          {#each ROOM_ICONS as ic (ic)}
            <button class="ic" class:sel={roomIcon(r) === ic} aria-label={ic} onclick={() => setIcon(i, ic)}><Icon name={ic} size={20} /></button>
          {/each}
        </li>
      {/if}
    {/each}
  </ul>
{/each}

<form
  class="add"
  onsubmit={(e) => {
    e.preventDefault();
    addRoom(newRoom);
  }}
>
  <input class="input" bind:value={newRoom} placeholder="Nouvelle pièce" />
  <button class="btn" type="submit" disabled={!newRoom.trim()}>Ajouter</button>
</form>
{#if presets.length}
  <div class="chips">
    {#each presets as p (p.name)}<button class="chip" onclick={() => addRoom(p.name, p.icon)}><Icon name={p.icon} size={15} /> {p.name}</button>{/each}
  </div>
{/if}

<h2>Groupes</h2>
<GroupsEditor />

{#if strays.length}
  <h2>Trouvées sur vos appareils</h2>
  <p class="muted">Ces pièces sont utilisées par des appareils mais pas encore dans la maison.</p>
  <div class="chips">
    {#each strays as s (s)}<button class="chip" onclick={() => addRoom(s)}><Icon name="plus" size={14} /> {s}</button>{/each}
  </div>
{/if}

{#if unsorted.length && layout.rooms.length}
  <h2>Appareils sans pièce</h2>
  <ul class="rows">
    {#each unsorted as d (d.key)}
      <li>
        <span class="dname">{d.name}</span>
        <select class="input floor" value="" onchange={(e) => place(d, e.currentTarget.value)} aria-label="Pièce de {d.name}">
          <option value="" disabled>Ranger dans…</option>
          {#each layout.rooms as r (r.name)}<option value={r.name}>{r.name}</option>{/each}
        </select>
      </li>
    {/each}
  </ul>
{/if}

<style>
  h2 {
    margin-top: 28px !important;
  }

  h3 {
    font-size: 14px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--muted);
    margin: 18px 0 6px;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 10px 0;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 7px 12px;
    border-radius: var(--radius-chip);
    background: var(--surface-2);
    font-weight: 600;
    font-size: 14px;
  }

  .chip.on {
    background: var(--ink);
    color: var(--bg);
  }

  .chip.on button {
    display: grid;
    color: inherit;
  }

  .add {
    display: flex;
    gap: 10px;
    margin: 10px 0;
    max-width: 460px;
  }

  .rooms,
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
    overflow: hidden;
  }

  .rooms li:not(.icons) {
    display: grid;
    grid-template-columns: 42px minmax(0, 1fr) auto auto auto;
    align-items: center;
    gap: 6px 10px;
    padding: 10px 12px;
  }

  .rooms li .floor {
    grid-column: 2 / -1;
    grid-row: 2;
    max-width: 220px;
  }

  .icons,
  .rows li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
  }

  .rooms li + li,
  .rows li + li {
    border-top: 1px solid var(--line);
  }

  .icons {
    flex-wrap: wrap;
    background: var(--surface-2);
  }

  .ic {
    display: grid;
    place-items: center;
    flex: none;
    width: 42px;
    height: 42px;
    border-radius: 12px;
    background: var(--surface-2);
    color: var(--ink);
  }

  .icons .ic {
    background: var(--surface);
  }

  .ic.sel {
    background: var(--ink);
    color: var(--bg);
  }

  .txt {
    flex: 1;
    display: grid;
    min-width: 0;
  }

  .name {
    text-align: left;
    font-weight: 600;
    font-size: 17px;
  }

  .txt span {
    font-size: 13px;
    color: var(--muted);
  }

  .floor {
    flex: 0 1 140px;
    min-height: 38px;
    padding: 4px 8px;
  }

  .mini {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--muted);
  }

  .mini:disabled {
    opacity: 0.35;
  }

  .dname {
    flex: 1;
    font-weight: 600;
  }
</style>
