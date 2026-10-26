<script>
  // L'accueil : un ou plusieurs tableaux de bord. On passe de l'un à l'autre en glissant
  // (petits points en bas, comme sur iPhone) ou par des onglets, au choix dans Apparence.
  // « Personnaliser » : sections, cartes de 1×1 à 4×4, déplacées au doigt, redimensionnées
  // à la poignée. Ce qu'un compte peut y faire dépend de ses droits (et de ceux de l'écran).
  import Tile from './Tile.svelte';
  import HouseSpecial from './HouseSpecial.svelte';
  import HouseListSheet from './HouseListSheet.svelte';
  import BoardsSheet from './BoardsSheet.svelte';
  import DeviceSheet from './DeviceSheet.svelte';
  import ForSheet from './ForSheet.svelte';
  import CardSheet from './CardSheet.svelte';
  import AddCardSheet from './AddCardSheet.svelte';
  import ConnectionForm from './ConnectionForm.svelte';
  import Icon from './Icon.svelte';
  import Sheet from './Sheet.svelte';
  import AlarmCard from './AlarmCard.svelte';
  import InfoSheet from './InfoSheet.svelte';
  import { press } from '../lib/press.js';
  import { domainOf, resolveAction } from '../lib/tiles.js';
  import { houseDefaults, houseEntity, resolveHouseAction } from '../lib/housecards.js';
  import { cardSize, sizeFromPixels } from '../lib/layout.js';
  import { effectiveLook } from '../lib/theme.js';
  import { describe as alarmView } from '../lib/security.js';
  import { roomIcon } from '../lib/rooms.js';
  import { dashboardFromRooms, deviceIndex, isOn } from '../lib/house.js';
  import {
    api,
    boardRights,
    command,
    currentBoard,
    home,
    isMember,
    loadSecurity,
    navBoards,
    saveBoard,
    saveBoards,
    showBoard,
    toast,
    turnOnFor,
  } from '../lib/store.svelte.js';

  let { onnavigate = null } = $props();

  const all = $derived(Object.values(home.entities));
  const index = $derived(deviceIndex(all));
  const board = $derived(currentBoard());
  const pages = $derived(navBoards());
  const rights = $derived(boardRights());
  const look = $derived(effectiveLook(home.look?.house, home.look?.mine, home.screen?.appearance));
  const order = $derived((home.layout?.rooms ?? []).map((r) => r.name));
  const zigbeeConfigured = $derived('zigbee2mqtt' in home.integrations);
  const empty = $derived(home.loaded && !all.some((e) => e.domain === 'light' || e.domain === 'switch' || e.domain === 'sensor'));
  const alarmUp = $derived(home.security && alarmView(home.security.status, home.now).tone !== 'off');
  const houseCtx = $derived({ entities: all, security: home.security, sun: home.sun, now: home.now });

  let editing = $state(false);
  let sheet = $state(null);

  // Un autre tableau, ou le droit de modifier retiré : on sort du mode modification.
  $effect(() => {
    board?.id;
    if (!rights.edit && !rights.add) editing = false;
  });

  /** Les sections du tableau : les siennes, ou rangées par pièce. */
  const raw = $derived(board ? (board.auto ? dashboardFromRooms(all, order).sections : board.sections) : []);

  /** Une carte prête à afficher : l'entité (vraie, ou calculée pour une carte Maison). */
  function resolve(card, s, i) {
    if (card.type === 'house') return { card, entity: houseEntity(card, houseCtx), device: null, section: s, index: i, house: true };
    const entity = card.entity_id ? home.entities[card.entity_id] : null;
    return { card, entity, device: card.entity_id ? (index.get(card.entity_id) ?? null) : null, section: s, index: i };
  }

  const sections = $derived(
    raw.map((sec, s) => ({
      title: sec.title,
      unsorted: board?.auto && sec.title === 'À ranger',
      cards: (sec.cards ?? [])
        .map((c, i) => resolve(c, s, i))
        .filter((c) => editing || c.card.type || (c.entity && !c.entity.hidden && !c.device?.hidden)),
    })),
  );

  // ---- Pièce = groupe ----
  function sectionLights(sec) {
    return sec.cards.filter((c) => !c.house && c.entity?.domain === 'light' && c.entity.state?.available !== false);
  }

  function toggleSection(sec) {
    const lights = sectionLights(sec);
    const anyOn = lights.some((c) => isOn(c.entity));
    for (const c of lights) command(c.entity.entity_id, anyOn ? 'turn_off' : 'turn_on');
  }

  // ---- Personnalisation ----
  function draft() {
    return $state.snapshot(raw);
  }

  function startEditing() {
    if (board.auto) saveBoard(board.id, dashboardFromRooms(all, order).sections);
    editing = true;
  }

  function mutate(fn) {
    const d = { sections: draft() };
    fn(d);
    saveBoard(board.id, d.sections);
  }

  async function resetToAuto() {
    if (!confirm(`Ranger « ${board.name} » tout seul par pièce ? Ses cartes seront effacées.`)) return;
    const list = $state.snapshot(home.boards.boards).map((b) => (b.id === board.id ? { ...b, auto: true, sections: [] } : b));
    if (await saveBoards(list)) editing = false;
  }

  function moveCard(from, to) {
    mutate((d) => {
      const [card] = d.sections[from.section].cards.splice(from.index, 1);
      let idx = to.index;
      if (from.section === to.section && from.index < to.index) idx -= 1;
      d.sections[to.section].cards.splice(Math.max(0, idx), 0, card);
    });
  }

  function moveSection(s, delta) {
    mutate((d) => {
      const t = s + delta;
      if (t < 0 || t >= d.sections.length) return;
      [d.sections[s], d.sections[t]] = [d.sections[t], d.sections[s]];
    });
  }

  // ---- Ce que font les cartes ----

  /** Le comportement par défaut, quand la carte n'en a pas choisi. */
  function tapOf(card) {
    if (card.tap) return card.tap;
    if (card.type === 'house') return houseDefaults(card.kind).tap;
    return ['light', 'switch', 'group', 'scene'].includes(domainOf(card.entity_id)) ? 'toggle' : 'details';
  }

  function holdOf(card) {
    if (card.hold) return card.hold;
    if (card.type === 'house') return houseDefaults(card.kind).hold;
    return domainOf(card.entity_id) === 'automation' && card.facet !== 'enabled' ? 'rearm' : 'details';
  }

  function openDetails(c) {
    if (c.house) {
      sheet = c.card.kind === 'alarm' ? { kind: 'alarm' } : { kind: 'house', card: c.card };
      return;
    }
    const d = domainOf(c.entity?.entity_id);
    if (c.device && ['light', 'switch', 'sensor', 'binary_sensor', 'siren'].includes(d)) {
      sheet = { kind: 'device', device: c.device, entity: c.entity };
    } else if (c.entity) {
      sheet = { kind: 'info', entityId: c.entity.entity_id };
    }
  }

  async function runHouse(action, c) {
    const r = resolveHouseAction(action, c.card, c.entity);
    if (r.kind === 'details') return openDetails(c);
    if (r.kind === 'rearm') {
      if (!isMember()) return toast('Réservé aux membres', 'alert');
      if (!r.automations.length) return toast('Rien à réarmer');
      await Promise.allSettled(r.automations.map((id) => api(`/automations/${id}/op`, { method: 'POST', body: { state: 'rearm' } })));
      return toast(r.automations.length > 1 ? `${r.automations.length} automatismes réarmés` : 'Réarmé');
    }
    if (r.kind !== 'commands') return;
    for (const x of r.commands) command(x.entity_id, x.action);
    if (r.arm && isMember()) {
      try {
        await api('/security/arm', { method: 'POST', body: { mode: r.arm } });
        loadSecurity();
      } catch (e) {
        toast(`Alarme : ${e.message}`, 'alert');
      }
    }
    const n = r.commands.length;
    if (!n && !r.arm) return toast(action === 'on' ? 'Tout est déjà allumé' : 'Tout est déjà éteint');
    if (c.card.kind === 'leave') toast(n ? `${n} appareil${n > 1 ? 's' : ''} coupé${n > 1 ? 's' : ''}. Bonne journée !` : 'Bonne journée !');
  }

  async function runAction(action, c) {
    if (c.house) return runHouse(action, c);
    const r = resolveAction(action, c.card, c.entity);
    if (r.kind === 'details') return openDetails(c);
    if (r.kind === 'command') {
      if (c.entity.state?.available === false) return toast(`${c.card.name || c.entity.name} ne répond pas`, 'alert');
      if (!isMember() && c.entity.entity_id.startsWith('siren.')) return;
      if (r.action !== 'turn_off' && c.card.for_min && !isOn(c.entity)) return turnOnFor(r.entity_id, c.card.for_min);
      return command(r.entity_id, r.action);
    }
    if (r.kind === 'automation') {
      if (!isMember()) return toast('Réservé aux membres', 'alert');
      try {
        await api(`/automations/${r.automation}/op`, { method: 'POST', body: r.body });
        if (r.body.state === 'rearm') toast(`${c.card.name || c.entity.name} : réarmé`);
      } catch (e) {
        toast(e.message, 'alert');
      }
    }
  }

  function addAndEdit(sectionIndex, card) {
    let pos = 0;
    mutate((d) => {
      if (!d.sections.length) d.sections.push({ title: 'Accueil', cards: [] });
      const s = Math.min(sectionIndex, d.sections.length - 1);
      d.sections[s].cards.push(card);
      pos = d.sections[s].cards.length - 1;
      sectionIndex = s;
    });
    if (!rights.edit) {
      sheet = null;
      return toast('Carte ajoutée');
    }
    // Tout de suite ses réglages : nom, appui long, règles…
    sheet = { kind: 'card', ref: resolve(card, sectionIndex, pos) };
  }

  // ---- Modifier l'accueil : appui long pour attraper une carte, sinon on fait défiler ----
  let drag = $state(null);
  let hold = $state.raw(null);
  const HOLD_DRAG_MS = 350;

  $effect(() => {
    // Pendant qu'une carte suit le doigt, la page ne défile pas.
    const stop = (e) => (drag || resize) && e.cancelable && e.preventDefault();
    document.addEventListener('touchmove', stop, { passive: false });
    return () => document.removeEventListener('touchmove', stop);
  });

  function slotDown(e, c) {
    if (!editing || !rights.edit || e.button > 0) return;
    const el = e.currentTarget;
    const h = { startX: e.clientX, startY: e.clientY, c, el, pointerId: e.pointerId, moved: false };
    h.timer = setTimeout(() => {
      if (hold !== h) return;
      drag = {
        from: { section: c.section, index: c.index },
        to: null,
        x: h.startX,
        y: h.startY,
        startX: h.startX,
        startY: h.startY,
        moved: true,
      };
      try {
        el.setPointerCapture(h.pointerId);
        navigator.vibrate?.(12);
      } catch {
        /* rien */
      }
    }, HOLD_DRAG_MS);
    hold = h;
  }

  function slotMove(e) {
    if (drag) return onDrag(e);
    if (hold && Math.hypot(e.clientX - hold.startX, e.clientY - hold.startY) > 10) {
      clearTimeout(hold.timer);
      hold.moved = true;
    }
  }

  function slotUp(c) {
    if (drag) {
      const { from, to } = drag;
      drag = null;
      hold = null;
      if (to) moveCard(from, { section: to.section, index: Math.min(to.index, 1e5) });
      return;
    }
    if (hold) {
      clearTimeout(hold.timer);
      const tapped = !hold.moved;
      hold = null;
      if (tapped) sheet = { kind: 'card', ref: c };
    }
  }

  function slotCancel() {
    if (hold) clearTimeout(hold.timer);
    hold = null;
    drag = null;
  }

  function onDrag(e) {
    drag.x = e.clientX;
    drag.y = e.clientY;
    // La carte déplacée suit le doigt : on regarde ce qu'il y a dessous, sans elle.
    const self = `${drag.from.section}:${drag.from.index}`;
    const under = document
      .elementsFromPoint(e.clientX, e.clientY)
      .find((n) => n.dataset?.slot && n.dataset.slot !== self);
    if (under) {
      const [section, index] = under.dataset.slot.split(':').map(Number);
      const r = under.getBoundingClientRect();
      const after = e.clientX > r.left + r.width / 2;
      drag.to = { section, index: index + (after ? 1 : 0) };
    } else {
      const sec = document.elementsFromPoint(e.clientX, e.clientY).find((n) => n.dataset?.section);
      drag.to = sec ? { section: Number(sec.dataset.section), index: 1e6 } : null;
    }
    // Près du bord : la page défile toute seule.
    if (e.clientY < 70) window.scrollBy(0, -12);
    else if (e.clientY > window.innerHeight - 90) window.scrollBy(0, 12);
  }

  // ---- Appui court, double appui ----
  // Sans double appui réglé, l'appui part tout de suite ; sinon on attend un peu le second.
  const DOUBLE_MS = 260;
  const taps = new Map();

  function onTap(c, key) {
    if (editing) return;
    const dbl = c.card.double;
    if (!dbl || dbl === 'none') return runAction(tapOf(c.card), c);
    const t = taps.get(key);
    if (t) {
      clearTimeout(t);
      taps.delete(key);
      return runAction(dbl, c);
    }
    taps.set(
      key,
      setTimeout(() => {
        taps.delete(key);
        runAction(tapOf(c.card), c);
      }, DOUBLE_MS),
    );
  }

  // ---- Redimensionner à la poignée (mode modification) ----
  let resize = $state(null);

  function gridMetrics(slot) {
    const grid = slot.parentElement;
    const cs = getComputedStyle(grid);
    const cols = cs.gridTemplateColumns.split(' ').filter(Boolean).length || 4;
    const gap = parseFloat(cs.columnGap) || 10;
    const row = parseFloat(cs.gridAutoRows) || 70;
    const unit = (grid.clientWidth - gap * (cols - 1)) / cols;
    return { cols, gap, row, unit };
  }

  function resizeDown(e, s, i, card) {
    e.stopPropagation();
    e.preventDefault();
    const slot = e.currentTarget.closest('.slot');
    const r = slot.getBoundingClientRect();
    const size = cardSize(card, home.entities[card.entity_id]);
    resize = { s, i, left: r.left, top: r.top, m: gridMetrics(slot), ...size };
    try {
      e.currentTarget.setPointerCapture(e.pointerId);
    } catch {
      /* rien */
    }
  }

  function resizeMove(e) {
    if (!resize) return;
    e.stopPropagation();
    const next = sizeFromPixels(e.clientX - resize.left, e.clientY - resize.top, resize.m);
    if (next.w !== resize.w || next.h !== resize.h) {
      resize.w = next.w;
      resize.h = next.h;
      navigator.vibrate?.(5);
    }
  }

  function resizeUp(e) {
    if (!resize) return;
    e.stopPropagation();
    const { s, i, w, h } = resize;
    resize = null;
    mutate((d) => {
      const card = d.sections[s].cards[i];
      delete card.size;
      Object.assign(card, { w, h });
    });
  }

  function sizeOf(c, s, i) {
    if (resize && resize.s === s && resize.i === i) return { w: resize.w, h: resize.h };
    return cardSize(c.card, c.entity);
  }

  // ---- Changer de tableau : glisser à gauche / à droite, ou onglets ----
  const pageIndex = $derived(pages.findIndex((b) => b.id === board?.id));
  let swipe = $state(null);
  let offset = $state(0);
  let animate = $state(false);

  function go(delta) {
    const next = pages[pageIndex + delta];
    if (!next) return;
    const w = window.innerWidth;
    animate = true;
    offset = -delta * w;
    setTimeout(() => {
      animate = false;
      showBoard(next.id);
      offset = delta * w * 0.35;
      scrollTo({ top: 0 });
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          animate = true;
          offset = 0;
        }),
      );
    }, 170);
  }

  function swipeStart(e) {
    if (editing || pages.length < 2 || look.nav !== 'swipe' || e.pointerType === 'mouse') return;
    if (e.target.closest('input, select, textarea, [data-noswipe], dialog')) return;
    swipe = { x: e.clientX, y: e.clientY, id: e.pointerId, on: false };
  }

  function swipeMove(e) {
    if (!swipe || e.pointerId !== swipe.id) return;
    const dx = e.clientX - swipe.x;
    const dy = e.clientY - swipe.y;
    if (!swipe.on) {
      if (Math.abs(dy) > 14 && Math.abs(dy) > Math.abs(dx)) return (swipe = null);
      if (Math.abs(dx) < 14 || Math.abs(dx) < Math.abs(dy) * 1.4) return;
      swipe.on = true;
    }
    // Au bout de la liste : ça résiste, comme sur un téléphone.
    const edge = (dx > 0 && pageIndex === 0) || (dx < 0 && pageIndex === pages.length - 1);
    animate = false;
    offset = edge ? dx * 0.25 : dx;
  }

  let swallow = false;

  function swipeEnd() {
    if (!swipe) return;
    const on = swipe.on;
    swipe = null;
    if (!on) return;
    swallow = true;
    setTimeout(() => (swallow = false), 80);
    const limit = Math.min(110, window.innerWidth * 0.22);
    if (offset < -limit && pageIndex < pages.length - 1) go(1);
    else if (offset > limit && pageIndex > 0) go(-1);
    else {
      animate = true;
      offset = 0;
    }
  }

  $effect(() => {
    // Après un glissement, le clic qui suit ne doit pas allumer une lampe.
    const stop = (e) => {
      if (swallow) {
        e.stopPropagation();
        e.preventDefault();
      }
    };
    document.addEventListener('click', stop, true);
    return () => document.removeEventListener('click', stop, true);
  });

  function onKey(e) {
    if (editing || sheet || e.target.closest?.('input, textarea, select')) return;
    if (e.key === 'ArrowRight') go(1);
    if (e.key === 'ArrowLeft') go(-1);
  }
</script>

<svelte:window onkeydown={onKey} />

{#if alarmUp && !editing}
  <AlarmCard />
{/if}

{#if empty && !zigbeeConfigured}
  <section class="setup">
    <h2>Connectez vos appareils Zigbee</h2>
    <p>Heim lit vos appareils via Zigbee2MQTT. Indiquez l'adresse de la machine où tourne le broker MQTT (souvent celle de Home Assistant).</p>
    <ConnectionForm />
  </section>
{/if}

{#if look.nav === 'tabs' && pages.length > 1 && !editing}
  <nav class="board-tabs" aria-label="Tableaux de bord">
    {#each pages as b (b.id)}
      <button class:on={b.id === board?.id} aria-current={b.id === board?.id ? 'page' : undefined} onclick={() => showBoard(b.id)}>
        <Icon name={b.icon || 'house'} size={17} />
        {b.name}
      </button>
    {/each}
  </nav>
{/if}

{#if home.loaded && board}
  <div class="bar">
    {#if editing}
      <button class="btn" onclick={() => (editing = false)}><Icon name="check" size={18} /> Terminé</button>
      {#if rights.add}<button class="btn quiet" onclick={() => (sheet = { kind: 'add' })}><Icon name="plus" size={18} /> Ajouter une carte</button>{/if}
      {#if rights.edit && rights.add}<button class="btn quiet" onclick={() => mutate((d) => d.sections.push({ title: 'Nouvelle section', cards: [] }))}>Nouvelle section</button>{/if}
      {#if rights.manage}
        <button class="btn quiet" onclick={() => (sheet = { kind: 'boards' })}><Icon name="grid" size={17} /> Tableaux</button>
        <button class="btn danger" onclick={resetToAuto}>Rangement automatique</button>
      {/if}
    {:else}
      {#if pages.length > 1 || board.hidden}<h1 class="board-name">{board.name}{#if board.hidden}<span class="hidden-tag">caché</span>{/if}</h1>{/if}
      {#if (rights.edit || rights.add) && (!board.auto || (rights.edit && rights.add))}
        <button class="link" onclick={startEditing}><Icon name="pencil" size={16} /> Personnaliser</button>
      {/if}
      {#if rights.manage}<button class="link" onclick={() => (sheet = { kind: 'boards' })}><Icon name="grid" size={16} /> Tableaux</button>{/if}
    {/if}
  </div>
{/if}

{#if editing}
  <p class="hint">
    {#if rights.edit}
      Appui long sur une carte pour la déplacer, poignée en bas à droite pour l’agrandir ou la réduire. Touchez-la pour ses réglages :
      fonction (appui court, long, double) et apparence (taille, disposition, couleurs…).
    {:else}
      Votre compte peut ajouter des cartes à ce tableau, sans modifier les autres.
    {/if}
  </p>
{/if}

<div
  class="pager"
  class:swipeable={look.nav === 'swipe' && pages.length > 1 && !editing}
  class:animate
  style:transform={offset ? `translateX(${offset}px)` : null}
  onpointerdown={swipeStart}
  onpointermove={swipeMove}
  onpointerup={swipeEnd}
  onpointercancel={swipeEnd}
  role="presentation"
>
  <div class="rooms" class:editing>
    {#each sections as sec, s (board?.id + ':' + s + sec.title)}
      {@const lights = sectionLights(sec)}
      <section class="room" class:unsorted={sec.unsorted} data-section={s}>
        {#if editing && rights.edit}
          <header>
            <input
              class="title-input"
              value={sec.title}
              aria-label="Nom de la section"
              onchange={(e) => mutate((d) => (d.sections[s].title = e.currentTarget.value.trim()))}
              placeholder="Sans titre"
            />
            <div class="sec-tools">
              <button class="mini" onclick={() => moveSection(s, -1)} disabled={s === 0} aria-label="Monter la section">↑</button>
              <button class="mini" onclick={() => moveSection(s, 1)} disabled={s === sections.length - 1} aria-label="Descendre la section">↓</button>
              <button
                class="mini"
                aria-label="Supprimer la section"
                onclick={() => confirm(`Supprimer la section « ${sec.title || 'sans titre'} » ? Les appareils restent dans Appareils.`) && mutate((d) => d.sections.splice(s, 1))}
              ><Icon name="close" size={16} /></button>
            </div>
          </header>
        {:else if sec.title}
          <header>
            <h2>
              {#if !sec.unsorted && board?.auto && sec.title !== 'Maison'}<span class="room-ic"><Icon name={roomIcon(home.layout?.rooms?.find((r) => r.name === sec.title) ?? { name: sec.title })} size={20} /></span>{/if}
              {sec.title}
            </h2>
            {#if sec.unsorted}
              <p>Touchez le crayon d'un appareil pour lui donner un nom et une pièce.</p>
            {:else if lights.length > 1}
              {@const lit = lights.some((c) => isOn(c.entity))}
              <button class="group" class:lit onclick={() => toggleSection(sec)}>
                {lit ? 'Éteindre la pièce' : 'Allumer la pièce'}
              </button>
            {/if}
          </header>
        {/if}

        <div class="grid">
          {#each sec.cards as c, i ((c.card.entity_id ?? `${c.card.type}:${c.card.kind ?? ''}`) + i)}
            {@const dragging = drag && drag.moved && drag.from.section === s && drag.from.index === i}
            {@const dropBefore = drag?.moved && drag.to?.section === s && drag.to?.index === i}
            {@const size = sizeOf(c, s, i)}
            {@const special = c.house && ['scenes', 'timers', 'clock'].includes(c.card.kind)}
            <div
              role="group"
              aria-label={c.card.name || c.entity?.name || c.card.text || 'Carte'}
              class="slot"
              class:dragging
              class:drop={dropBefore}
              class:lifted={hold && !drag && editing}
              class:resizing={resize?.s === s && resize?.i === i}
              style:grid-column="span {size.w}"
              style:grid-row="span {size.h}"
              data-slot="{s}:{i}"
              onpointerdown={(e) => slotDown(e, { ...c, section: s, index: i })}
              onpointermove={slotMove}
              onpointerup={() => slotUp({ ...c, section: s, index: i })}
              onpointercancel={slotCancel}
              use:press={{ disabled: editing || !c.entity || special, onhold: () => runAction(holdOf(c.card), c) }}
              style:transform={dragging ? `translate(${drag.x - drag.startX}px, ${drag.y - drag.startY}px) scale(1.03)` : null}
            >
              {#if c.card.type === 'title'}
                <div class="title-card" class:center={c.card.align === 'center'}>
                  <h3>{c.card.text || 'Titre'}</h3>
                  {#if c.card.subtitle}<p>{c.card.subtitle}</p>{/if}
                </div>
              {:else if c.card.type === 'spacer'}
                <div class="spacer" aria-hidden="true">{#if editing}Espace{/if}</div>
              {:else if special}
                <HouseSpecial card={c.card} preview={editing} />
              {:else if !c.entity}
                <div class="ghost">Introuvable (supprimé ?)</div>
              {:else}
                <Tile
                  card={c.card.w || c.card.h ? c.card : { ...c.card, ...size }}
                  entity={c.entity}
                  device={c.device}
                  ontap={() => onTap(c, `${s}:${i}`)}
                  onfor={editing || c.house ? null : () => (sheet = { kind: 'for', entity: c.entity })}
                />
              {/if}
              {#if editing && rights.edit}
                <span class="veil" aria-hidden="true"></span>
                <span class="size-badge num" aria-hidden="true">{size.w}×{size.h}</span>
                <button
                  class="grip"
                  aria-label="Redimensionner"
                  onpointerdown={(e) => resizeDown(e, s, i, c.card)}
                  onpointermove={resizeMove}
                  onpointerup={resizeUp}
                  onpointercancel={() => (resize = null)}
                  onclick={(e) => e.stopPropagation()}
                ></button>
              {:else if editing}
                <span class="veil locked" aria-hidden="true"></span>
              {/if}
            </div>
          {/each}
          {#if editing && rights.add && !sec.cards.length}
            <button class="empty-slot" onclick={() => (sheet = { kind: 'add', section: s })}>Ajouter une carte ici</button>
          {/if}
        </div>
      </section>
    {:else}
      {#if home.loaded && board}
        <p class="nothing">Ce tableau est vide.{#if rights.add} Touchez « Personnaliser » pour y mettre des cartes.{/if}</p>
      {/if}
    {/each}
  </div>
</div>

{#if pages.length > 1 && look.nav === 'swipe' && !editing}
  <div class="dots" class:kiosk={home.screen?.kiosk} role="tablist" aria-label="Tableaux de bord">
    {#each pages as b, k (b.id)}
      <button
        role="tab"
        class:on={b.id === board?.id}
        aria-selected={b.id === board?.id}
        aria-label={b.name}
        title={b.name}
        onclick={() => (k === pageIndex ? null : go(k - pageIndex))}
      ></button>
    {/each}
  </div>
{/if}

{#if sheet?.kind === 'device'}
  <DeviceSheet device={sheet.device} entity={sheet.entity} onclose={() => (sheet = null)} />
{:else if sheet?.kind === 'info'}
  <InfoSheet entityId={sheet.entityId} onclose={() => (sheet = null)} onopen={onnavigate ? (id) => ((sheet = null), onnavigate(`automatismes/${id}`)) : null} />
{:else if sheet?.kind === 'for'}
  <ForSheet entity={sheet.entity} onclose={() => (sheet = null)} />
{:else if sheet?.kind === 'house'}
  <HouseListSheet card={sheet.card} onclose={() => (sheet = null)} />
{:else if sheet?.kind === 'alarm'}
  <Sheet title="Alarme" onclose={() => (sheet = null)}><AlarmCard /></Sheet>
{:else if sheet?.kind === 'boards'}
  <BoardsSheet onclose={() => (sheet = null)} />
{:else if sheet?.kind === 'card'}
  <CardSheet ref={sheet.ref} sections={raw} onchange={(fn) => mutate(fn)} onclose={() => (sheet = null)} />
{:else if sheet?.kind === 'add'}
  <AddCardSheet dashboard={{ sections: raw }} section={sheet.section ?? 0} onadd={addAndEdit} onclose={() => (sheet = null)} />
{/if}

<style>
  .room-ic {
    display: inline-grid;
    place-items: center;
    width: 34px;
    height: 34px;
    margin-right: 6px;
    border-radius: 10px;
    background: var(--surface-2);
    color: var(--muted);
    vertical-align: middle;
  }

  .setup {
    padding: 20px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
    margin-bottom: 20px;
  }

  .setup h2 {
    margin: 0 0 6px;
    font-size: 22px;
  }

  .setup p {
    margin: 0 0 16px;
    color: var(--muted);
  }

  .bar {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 8px 0 0;
  }

  .link {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 40px;
    color: var(--muted);
    font-size: 15px;
  }

  .hint {
    color: var(--muted);
    font-size: 15px;
  }

  .room {
    padding-top: 28px;
  }

  @media (min-width: 960px) {
    .rooms {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      column-gap: 40px;
      align-items: start;
    }

    .unsorted {
      grid-column: 1 / -1;
    }
  }

  .room header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
    margin-bottom: 12px;
  }

  .room h2 {
    margin: 0;
    font-size: 26px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .room header p {
    margin: 0;
    font-size: 15px;
    color: var(--muted);
  }

  .group {
    font-size: 15px;
    font-weight: 600;
    color: var(--muted);
    min-height: 36px;
  }

  .group.lit {
    color: color-mix(in srgb, var(--ember) 70%, var(--ink));
  }

  .unsorted {
    border-top: 1px dashed var(--line);
    margin-top: 28px;
  }

  .unsorted header {
    display: grid;
  }

  /* Grille en petites cases : 4 de large sur téléphone, 6 ou 8 quand la place le permet.
     Une case fait 70 px de haut : une tuile d'avant = 2×2. */
  .room {
    container-type: inline-size;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    grid-auto-rows: var(--row, 70px);
    grid-auto-flow: row dense;
    gap: var(--gap, 10px);
  }

  @container (min-width: 640px) {
    .grid {
      grid-template-columns: repeat(6, minmax(0, 1fr));
    }
  }

  @container (min-width: 900px) {
    .grid {
      grid-template-columns: repeat(8, minmax(0, 1fr));
    }
  }

  .slot {
    position: relative;
    min-width: 0;
    min-height: 0;
  }

  .slot.resizing {
    z-index: 4;
    outline: 2px solid var(--ember);
    outline-offset: 2px;
    border-radius: var(--radius-tile);
  }

  .grip {
    position: absolute;
    right: -4px;
    bottom: -4px;
    z-index: 3;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    background: var(--ink);
    box-shadow: var(--shadow);
    cursor: nwse-resize;
    touch-action: none;
  }

  .grip::after {
    content: '';
    position: absolute;
    inset: 9px;
    border-right: 2px solid var(--bg);
    border-bottom: 2px solid var(--bg);
  }

  .size-badge {
    position: absolute;
    top: 6px;
    left: 8px;
    z-index: 3;
    padding: 0 6px;
    border-radius: 8px;
    font-size: 11px;
    font-weight: 700;
    background: color-mix(in srgb, var(--ink) 75%, transparent);
    color: var(--bg);
    pointer-events: none;
  }

  .title-card {
    height: 100%;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    padding: 0 4px 4px;
  }

  .title-card.center {
    align-items: center;
    text-align: center;
  }

  .title-card h3 {
    margin: 0;
    font-size: 20px;
    font-weight: 650;
    letter-spacing: -0.01em;
  }

  .title-card p {
    margin: 2px 0 0;
    font-size: 14px;
    color: var(--muted);
  }

  .spacer {
    height: 100%;
    display: grid;
    place-items: center;
    font-size: 12px;
    color: var(--muted);
  }

  .editing .slot {
    cursor: grab;
    /* On fait défiler normalement ; c'est l'appui long qui attrape la carte. */
    touch-action: manipulation;
    user-select: none;
    -webkit-user-select: none;
    -webkit-touch-callout: none;
    animation: wiggle 0.3s ease-in-out;
  }

  .slot.lifted {
    transition: transform 0.35s ease;
    transform: scale(0.97);
  }

  @keyframes wiggle {
    40% {
      transform: rotate(-0.6deg);
    }
    80% {
      transform: rotate(0.6deg);
    }
  }

  .veil {
    position: absolute;
    inset: 0;
    border-radius: var(--radius-tile);
    outline: 2px dashed var(--line);
    outline-offset: -2px;
  }

  .slot.dragging {
    z-index: 5;
    opacity: 0.85;
    cursor: grabbing;
    animation: none;
  }

  .slot.drop::before {
    content: '';
    position: absolute;
    left: -6px;
    top: 8px;
    bottom: 8px;
    width: 3px;
    border-radius: 2px;
    background: var(--ember);
  }

  .ghost,
  .empty-slot {
    display: grid;
    place-items: center;
    height: 100%;
    min-height: 70px;
    border-radius: var(--radius-tile);
    border: 1.5px dashed var(--line);
    color: var(--muted);
    font-size: 14px;
  }

  .empty-slot {
    grid-column: 1 / -1;
  }

  .title-input {
    font: inherit;
    font-size: 22px;
    font-weight: 600;
    padding: 6px 10px;
    border-radius: 10px;
    border: 1px solid var(--line);
    background: var(--surface);
    min-width: 0;
    flex: 1;
  }

  .sec-tools {
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
    font-weight: 600;
  }

  .mini:disabled {
    opacity: 0.4;
  }

  .board-tabs {
    display: flex;
    gap: 6px;
    overflow-x: auto;
    padding: 2px 2px 12px;
    margin: 0 -2px 4px;
    scrollbar-width: none;
  }

  .board-tabs button {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 7px;
    min-height: 40px;
    padding: 0 16px;
    border-radius: var(--radius-chip);
    background: var(--surface-2);
    color: var(--muted);
    font-weight: 600;
  }

  .board-tabs button.on {
    background: var(--ink);
    color: var(--bg);
  }

  .board-name {
    margin: 0 auto 0 0;
    font-size: 26px;
    font-weight: 650;
    letter-spacing: -0.01em;
  }

  .hidden-tag {
    margin-left: 8px;
    padding: 2px 8px;
    border-radius: 8px;
    font-size: 12px;
    font-weight: 600;
    vertical-align: middle;
    background: var(--surface-2);
    color: var(--muted);
  }

  .bar {
    align-items: center;
  }

  .pager {
    min-height: 50vh;
  }

  .pager.swipeable {
    touch-action: pan-y;
  }

  .pager.animate {
    transition: transform 0.17s ease-out;
  }

  .nothing {
    color: var(--muted);
    padding: 40px 0;
    text-align: center;
  }

  .veil.locked {
    outline-style: solid;
    opacity: 0.5;
  }

  /* Les petits points, comme sur iPhone : au-dessus de la barre d'onglets. */
  .dots {
    position: fixed;
    left: 50%;
    bottom: calc(80px + env(safe-area-inset-bottom));
    transform: translateX(-50%);
    z-index: 9;
    display: flex;
    gap: 2px;
    padding: 5px 8px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--surface) 75%, transparent);
    backdrop-filter: blur(10px);
    -webkit-backdrop-filter: blur(10px);
  }

  .dots.kiosk {
    bottom: calc(16px + env(safe-area-inset-bottom));
  }

  .dots button {
    width: 18px;
    height: 18px;
    display: grid;
    place-items: center;
  }

  .dots button::after {
    content: '';
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--muted);
    opacity: 0.45;
    transition: transform 0.2s ease, opacity 0.2s ease;
  }

  .dots button.on::after {
    background: var(--ink);
    opacity: 1;
    transform: scale(1.15);
  }

  @media (min-width: 760px) {
    .dots {
      bottom: 22px;
    }
  }
</style>
