<script>
  // L'éditeur de scène : on choisit les appareils, on règle chacun (allumé, luminosité, blanc,
  // couleur, effet), avec l'aperçu en direct si on veut voir le résultat sur les vraies lampes.
  // En quittant l'éditeur (enregistré ou non), les lampes reviennent comme avant l'aperçu.
  import { onDestroy, untrack } from 'svelte';
  import Sheet from './Sheet.svelte';
  import Icon from './Icon.svelte';
  import SceneDevice from './SceneDevice.svelte';
  import { COLORS, COLOR_NAMES, TILE_ICONS } from '../lib/tiles.js';
  import { groupControls } from '../lib/controls.js';
  import { cleanTarget, sceneCandidates, templateTarget, TEMPLATES } from '../lib/scenes.js';
  import { slug } from '../lib/house.js';
  import { api, home, homeRooms, toast } from '../lib/store.svelte.js';

  /** scene : la scène à modifier (null = nouvelle) ; template : un modèle de départ. */
  let { scene = null, template = null, room: startRoom = '', onclose, onsaved, onduplicate = null, ondelete = null } = $props();

  const initial = untrack(() => scene);
  let name = $state(initial?.name ?? untrack(() => template?.name) ?? '');
  let room = $state(initial?.room ?? untrack(() => startRoom) ?? '');
  let icon = $state(initial?.icon ?? untrack(() => template?.icon) ?? '');
  let color = $state(initial?.color ?? untrack(() => template?.color) ?? '');
  let transition = $state(initial?.transition_s ?? 0);
  let offMode = $state(initial?.off_mode ?? 'turn_off');
  /** Ordre d'affichage + réglage de chaque appareil. */
  let order = $state(Object.keys(initial?.targets ?? {}));
  let targets = $state(structuredClone(initial?.targets ?? {}));
  let openId = $state(null);
  let picking = $state(!initial && !untrack(() => template));
  let pickQuery = $state('');
  let pickRoom = $state(untrack(() => startRoom) || '');
  let live = $state(false);
  let touched = false;
  let saving = $state(false);
  let showLook = $state(false);

  const all = $derived(Object.values(home.entities));
  const candidates = $derived(sceneCandidates(all));
  const pickList = $derived(
    candidates.filter(
      (e) =>
        !order.includes(e.entity_id) &&
        (!pickRoom || (e.room || '') === pickRoom) &&
        (!pickQuery.trim() || `${e.name} ${e.room ?? ''}`.toLowerCase().includes(pickQuery.trim().toLowerCase())),
    ),
  );

  // Les réglages « lumière » de chaque appareil (couleur, effets) : demandés une fois.
  let lightCtl = $state({});
  async function loadControls(entityId) {
    const e = home.entities[entityId];
    const dev = e?.device_id;
    // Les appareils de démo n'ont pas de réglages détaillés.
    if (!dev || dev.startsWith('virtual:') || e.domain !== 'light' || lightCtl[entityId] !== undefined) return;
    lightCtl[entityId] = null;
    try {
      const c = await api(`/devices/${encodeURIComponent(dev)}/controls`);
      lightCtl[entityId] = groupControls(c.controls ?? []).light;
    } catch {
      /* appareil sans réglages détaillés */
    }
  }
  $effect(() => {
    for (const id of order) untrack(() => loadControls(id));
  });

  // Un modèle : les lampes de la pièce choisie, réglées comme le modèle le propose.
  if (untrack(() => template)) {
    const tpl = untrack(() => template);
    const r = untrack(() => startRoom);
    const lamps = untrack(() => candidates).filter((e) => e.domain !== 'group' && (!r || (e.room || '') === r));
    for (const e of lamps) {
      order.push(e.entity_id);
      targets[e.entity_id] = templateTarget(tpl, e);
    }
  }

  // ---- Aperçu en direct ----
  let pending = {};
  let timer;
  function pushPreview(ids) {
    if (!live) return;
    for (const id of ids) pending[id] = cleanTarget($state.snapshot(targets[id]));
    clearTimeout(timer);
    timer = setTimeout(async () => {
      const batch = pending;
      pending = {};
      touched = true;
      try {
        await api('/scenes/preview', { method: 'POST', body: { targets: batch } });
      } catch (e) {
        toast(e.message, 'alert');
      }
    }, 200);
  }

  function setLive(on) {
    live = on;
    if (on) pushPreview(order);
  }

  async function endPreview() {
    clearTimeout(timer);
    if (!touched) return;
    touched = false;
    try {
      await api('/scenes/preview/end', { method: 'POST' });
    } catch {
      /* Heim remettra tout comme avant tout seul au bout de 10 min */
    }
  }

  onDestroy(() => {
    endPreview();
  });

  // ---- Appareils ----
  async function add(ids) {
    const fresh = ids.filter((id) => !order.includes(id));
    if (!fresh.length) return;
    // On part de l'état actuel de chaque appareil : c'est souvent déjà presque ça.
    let snap = {};
    try {
      snap = await api('/scenes/snapshot', { method: 'POST', body: { entities: fresh } });
    } catch {
      /* injoignable : on part de « allumé » */
    }
    for (const id of fresh) {
      targets[id] = snap[id] ?? { on: true };
      order.push(id);
    }
    if (fresh.length === 1) openId = fresh[0];
    pushPreview(fresh);
  }

  function remove(id) {
    order = order.filter((x) => x !== id);
    delete targets[id];
    if (openId === id) openId = null;
  }

  async function captureAll() {
    if (!order.length) return;
    try {
      const snap = await api('/scenes/snapshot', { method: 'POST', body: { entities: order } });
      for (const [id, t] of Object.entries(snap)) targets[id] = t;
      toast('Réglages repris de l’état actuel');
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  async function tryIt() {
    touched = true;
    try {
      const batch = Object.fromEntries(order.map((id) => [id, cleanTarget($state.snapshot(targets[id]))]));
      await api('/scenes/preview', { method: 'POST', body: { targets: batch, transition_s: transition || null } });
      toast('Essai en cours : tout reviendra comme avant en quittant');
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  async function save() {
    if (!name.trim() || !order.length) return;
    saving = true;
    try {
      const body = {
        name: name.trim(),
        room: room || null,
        icon: icon || null,
        color: color || null,
        transition_s: transition || null,
        off_mode: offMode,
        targets: Object.fromEntries(order.map((id) => [id, cleanTarget($state.snapshot(targets[id]))])),
      };
      const id = initial?.id ?? (slug(name) || 'scene');
      const saved = await api(`/scenes/${id}`, { method: 'PUT', body });
      toast(`Scène « ${saved.name} » enregistrée`);
      onsaved?.(saved);
      onclose();
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      saving = false;
    }
  }

  const FADES = [0, 1, 2, 3, 5, 10];
</script>

<Sheet title={initial ? initial.name : template ? `Nouvelle scène : ${template.name}` : 'Nouvelle scène'} {onclose}>
  <div class="livebar" class:on={live}>
    <label class="switchline">
      <span class="sw" class:on={live}><input type="checkbox" checked={live} onchange={(e) => setLive(e.currentTarget.checked)} /><i></i></span>
      <span>
        <strong>Aperçu en direct</strong>
        <small>{live ? 'Chaque réglage s’applique tout de suite sur les lampes.' : 'Voir le résultat sur les vraies lampes pendant le réglage.'}</small>
      </span>
    </label>
  </div>

  <label class="field">
    <span>Nom</span>
    <input class="input" bind:value={name} placeholder="Soirée, Ciné, Dîner…" autocomplete="off" />
  </label>

  <h3>Appareils <span class="n">{order.length}</span></h3>
  {#if order.length}
    <ul class="devs">
      {#each order as id (id)}
        <SceneDevice
          entity={home.entities[id]}
          bind:target={targets[id]}
          light={lightCtl[id] ?? null}
          open={openId === id}
          ontoggle={() => (openId = openId === id ? null : id)}
          onchange={() => pushPreview([id])}
          onremove={() => remove(id)}
        />
      {/each}
    </ul>
    <div class="row">
      <button class="btn quiet" onclick={() => (picking = !picking)}><Icon name="plus" size={16} /> Ajouter des appareils</button>
      <button class="btn quiet" onclick={captureAll}><Icon name="restart" size={16} /> Reprendre l'état actuel</button>
    </div>
  {:else}
    <p class="muted">Choisissez les lumières, prises ou groupes de la scène. Chacun part de son état actuel, puis se règle.</p>
  {/if}

  {#if picking}
    <div class="picker">
      <div class="chips">
        <button class="chip" class:on={pickRoom === ''} onclick={() => (pickRoom = '')}>Toutes les pièces</button>
        {#each homeRooms() as r (r)}<button class="chip" class:on={pickRoom === r} onclick={() => (pickRoom = r)}>{r}</button>{/each}
      </div>
      <input class="input" type="search" bind:value={pickQuery} placeholder="Chercher un appareil" aria-label="Chercher" />
      {#if pickList.length}
        <ul class="pick">
          {#each pickList as e (e.entity_id)}
            <li>
              <button onclick={() => add([e.entity_id])}>
                <Icon name={e.domain === 'switch' ? 'plug' : 'bulb'} size={18} />
                <span>{e.name}{e.domain === 'group' ? ' (groupe)' : ''}</span>
                <small>{e.room || 'Sans pièce'} · {e.state?.value === 'on' ? 'allumé' : 'éteint'}</small>
                <Icon name="plus" size={16} />
              </button>
            </li>
          {/each}
        </ul>
        {#if pickList.length > 1}
          <button class="btn quiet" onclick={() => add(pickList.map((e) => e.entity_id))}>Tout ajouter ({pickList.length})</button>
        {/if}
      {:else}
        <p class="muted">Rien d'autre à ajouter ici.</p>
      {/if}
    </div>
  {/if}

  <details class="opts">
    <summary>Comportement</summary>
    <div class="field">
      <span>Fondu à l'activation</span>
      <div class="seg wrap">
        {#each FADES as f (f)}<button class:on={transition === f} onclick={() => (transition = f)}>{f ? `${f} s` : 'Aucun'}</button>{/each}
      </div>
    </div>
    <div class="field">
      <span>Quand on la désactive</span>
      <div class="seg wrap">
        <button class:on={offMode === 'turn_off'} onclick={() => (offMode = 'turn_off')}>Tout éteindre</button>
        <button class:on={offMode === 'restore'} onclick={() => (offMode = 'restore')}>Revenir comme avant</button>
      </div>
    </div>
    <label class="field">
      <span>Pièce de la scène</span>
      <select class="input" bind:value={room}>
        <option value="">Aucune</option>
        {#each homeRooms() as r (r)}<option value={r}>{r}</option>{/each}
      </select>
    </label>
  </details>

  <details class="opts" bind:open={showLook}>
    <summary>Apparence sur l'accueil</summary>
    <div class="icons">
      <button class="ic" class:sel={!icon} onclick={() => (icon = '')} title="Icône par défaut"><Icon name="scene" size={20} /></button>
      {#each TILE_ICONS as ic (ic)}
        <button class="ic" class:sel={icon === ic} onclick={() => (icon = ic)} aria-label={ic}><Icon name={ic} size={20} /></button>
      {/each}
    </div>
    <div class="colors">
      <button class="dot none" class:sel={!color} onclick={() => (color = '')} aria-label="Couleur par défaut"></button>
      {#each Object.entries(COLORS) as [id, hex] (id)}
        <button class="dot" class:sel={color === hex} style:background={hex} onclick={() => (color = hex)} title={COLOR_NAMES[id]} aria-label={COLOR_NAMES[id]}></button>
      {/each}
    </div>
  </details>

  <div class="actions">
    <button class="btn" onclick={save} disabled={saving || !name.trim() || !order.length}>Enregistrer</button>
    {#if order.length && !live}<button class="btn quiet" onclick={tryIt}><Icon name="play" size={15} /> Essayer</button>{/if}
    {#if initial && onduplicate}<button class="btn quiet" onclick={() => onduplicate(initial.id)}><Icon name="copy" size={15} /> Dupliquer</button>{/if}
    {#if initial && ondelete}<button class="btn danger" onclick={() => ondelete(initial)}>Supprimer</button>{/if}
  </div>
  {#if !initial && !template}
    <p class="muted small">Ou partez d'un modèle : {TEMPLATES.map((t) => t.name).join(', ')} (bouton « Modèles » de la page Scènes).</p>
  {/if}
</Sheet>

<style>
  .livebar {
    position: sticky;
    top: -2px;
    z-index: 2;
    margin: -4px 0 12px;
    padding: 10px 12px;
    border-radius: 16px;
    background: var(--surface-2);
    transition: background-color 0.25s ease;
  }

  .livebar.on {
    background: color-mix(in srgb, var(--ember) 35%, var(--surface));
  }

  .switchline {
    display: flex;
    align-items: center;
    gap: 12px;
    cursor: pointer;
  }

  .switchline small {
    display: block;
    font-size: 13px;
    color: var(--muted);
  }

  .sw {
    position: relative;
    flex: none;
    width: 52px;
    height: 32px;
    border-radius: 16px;
    background: var(--line);
    transition: background 0.15s;
  }

  .sw input {
    position: absolute;
    inset: 0;
    opacity: 0;
    margin: 0;
    cursor: pointer;
  }

  .sw i {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.25);
    transition: transform 0.15s;
    pointer-events: none;
  }

  .sw.on {
    background: var(--moss);
  }

  .sw.on i {
    transform: translateX(20px);
  }

  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 18px 0 8px;
    font-size: 18px;
  }

  .n {
    min-width: 22px;
    padding: 0 7px;
    border-radius: 11px;
    background: var(--ink);
    color: var(--bg);
    font-size: 12px;
    text-align: center;
  }

  .devs,
  .pick {
    list-style: none;
    margin: 0 0 10px;
    padding: 0;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
    overflow: hidden;
  }

  .row,
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .actions {
    margin-top: 16px;
  }

  .picker {
    display: grid;
    gap: 8px;
    margin: 10px 0;
    padding: 12px;
    border-radius: 18px;
    border: 1px solid var(--line);
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

  .pick {
    max-height: 300px;
    overflow-y: auto;
  }

  .pick li + li {
    border-top: 1px solid var(--line);
  }

  .pick button {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 10px 12px;
    text-align: left;
  }

  .pick span {
    flex: 1;
    font-weight: 600;
  }

  .pick small {
    color: var(--muted);
  }

  .opts {
    border-top: 1px solid var(--line);
    padding: 10px 0;
    margin-top: 8px;
  }

  .opts summary {
    cursor: pointer;
    font-weight: 700;
    font-size: 17px;
    padding: 4px 0 8px;
  }

  .seg {
    display: flex;
    gap: 6px;
  }

  .seg.wrap {
    flex-wrap: wrap;
  }

  .seg button {
    min-height: 40px;
    padding: 0 14px;
    border-radius: var(--radius-chip);
    border: 1.5px solid var(--line);
    font-weight: 600;
  }

  .seg button.on {
    background: var(--ink);
    border-color: var(--ink);
    color: var(--bg);
  }

  .icons,
  .colors {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 10px;
  }

  .ic {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: 12px;
    background: var(--surface-2);
  }

  .ic.sel {
    background: var(--ink);
    color: var(--bg);
  }

  .dot {
    width: 30px;
    height: 30px;
    border-radius: 50%;
    border: 2px solid transparent;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.12);
  }

  .dot.none {
    background: repeating-linear-gradient(45deg, var(--surface-2) 0 4px, var(--line) 4px 6px);
  }

  .dot.sel {
    border-color: var(--ink);
    transform: scale(1.08);
  }

  .small {
    font-size: 13px;
  }
</style>
