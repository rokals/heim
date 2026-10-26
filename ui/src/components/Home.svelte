<script>
  // L'accueil. Par défaut, rangé tout seul par pièce. « Personnaliser » le transforme en
  // tableau de bord à soi : sections, cartes, taille, minuterie d'un clic, ordre au doigt.
  import ControlTile from './ControlTile.svelte';
  import SensorCard from './SensorCard.svelte';
  import TimerTile from './TimerTile.svelte';
  import DeviceSheet from './DeviceSheet.svelte';
  import ForSheet from './ForSheet.svelte';
  import CardSheet from './CardSheet.svelte';
  import AddCardSheet from './AddCardSheet.svelte';
  import ConnectionForm from './ConnectionForm.svelte';
  import Icon from './Icon.svelte';
  import AlarmCard from './AlarmCard.svelte';
  import ButtonTile from './ButtonTile.svelte';
  import InfoSheet from './InfoSheet.svelte';
  import { press } from '../lib/press.js';
  import { appearance, domainOf, resolveAction, sinceText } from '../lib/tiles.js';
  import { describe as alarmView, isConfigured } from '../lib/security.js';
  import { roomIcon } from '../lib/rooms.js';
  import { buildRooms, dashboardFromRooms, deviceIndex, houseSummary, isOn, UNSORTED } from '../lib/house.js';
  import { api, command, home, isAdmin, isMember, saveDashboard, toast, turnOnFor } from '../lib/store.svelte.js';

  let { onnavigate = null } = $props();

  const all = $derived(Object.values(home.entities));
  const index = $derived(deviceIndex(all));
  const summary = $derived(houseSummary(all));
  const scenes = $derived(
    all.filter((e) => e.domain === 'scene' && !e.hidden).sort((a, b) => a.name.localeCompare(b.name, 'fr')),
  );
  const running = $derived(
    all.filter((e) => e.domain === 'timer' && !e.hidden && ['running', 'paused', 'finished'].includes(e.state?.value)),
  );
  const custom = $derived(home.dashboard?.sections ? home.dashboard : null);
  // Automatismes « levés » (courrier arrivé, lessive finie…) : en haut, jusqu'au réarmement.
  const fired = $derived(
    all
      .filter((e) => e.domain === 'automation' && e.state?.attributes?.fired && e.state?.attributes?.enabled !== false)
      .sort((a, b) => a.name.localeCompare(b.name, 'fr')),
  );
  const zigbeeConfigured = $derived('zigbee2mqtt' in home.integrations);
  const empty = $derived(home.loaded && !all.some((e) => e.domain === 'light' || e.domain === 'switch' || e.domain === 'sensor'));

  let editing = $state(false);
  let sheet = $state(null);

  /** Une carte prête à afficher : l'entité, son appareil, et ses réglages. */
  function resolve(card, s, i) {
    const entity = home.entities[card.entity_id];
    return { card, entity, device: index.get(card.entity_id) ?? null, section: s, index: i };
  }

  const sections = $derived.by(() => {
    if (custom) {
      return custom.sections.map((sec, s) => ({
        title: sec.title,
        cards: sec.cards
          .map((c, i) => resolve(c, s, i))
          .filter((c) => editing || (c.entity && !c.entity.hidden && !c.device?.hidden)),
        timers: [],
      }));
    }
    return buildRooms(all, (home.layout?.rooms ?? []).map((r) => r.name)).map((r) => ({
      title: r.name === UNSORTED ? 'À ranger' : r.name,
      unsorted: r.name === UNSORTED,
      cards: [
        ...r.controls.map((c) => ({ card: { entity_id: c.entity.entity_id, size: 1 }, entity: c.entity, device: c.device })),
        ...r.sensors.map((d) => ({ card: { entity_id: d.primary.entity_id, size: 2 }, entity: d.primary, device: d })),
      ],
      timers: r.timers.filter((t) => !running.includes(t)),
    }));
  });

  const headline = $derived.by(() => {
    const n = summary.lightsOn.length;
    if (!summary.lights.length && !summary.plugs.length) return 'Bienvenue';
    if (n === 0) return 'Tout est éteint';
    return n === 1 ? 'Une lumière allumée' : `${n} lumières allumées`;
  });

  const people = $derived(all.filter((e) => e.domain === 'person').sort((a, b) => a.name.localeCompare(b.name, 'fr')));
  const atHome = $derived(people.filter((p) => p.state?.value === 'home'));

  const litRooms = $derived(
    [...new Set(summary.lightsOn.map((e) => e.room).filter(Boolean))].sort((a, b) => a.localeCompare(b, 'fr')),
  );

  function listNames(names) {
    if (names.length <= 1) return names.join('');
    return names.slice(0, -1).join(', ') + ' et ' + names.at(-1);
  }

  // ---- Pièce = groupe ----
  function sectionLights(sec) {
    return sec.cards.filter((c) => c.entity?.domain === 'light' && c.entity.state?.available !== false);
  }

  function toggleSection(sec) {
    const lights = sectionLights(sec);
    const anyOn = lights.some((c) => isOn(c.entity));
    for (const c of lights) command(c.entity.entity_id, anyOn ? 'turn_off' : 'turn_on');
  }

  // ---- Personnalisation ----
  function draft() {
    return $state.snapshot(custom ?? dashboardFromRooms(all, (home.layout?.rooms ?? []).map((r) => r.name)));
  }

  function startEditing() {
    if (!custom) saveDashboard(dashboardFromRooms(all, (home.layout?.rooms ?? []).map((r) => r.name)));
    editing = true;
  }

  function mutate(fn) {
    const d = draft();
    fn(d);
    saveDashboard(d);
  }

  function resetToAuto() {
    if (!confirm("Revenir au rangement automatique par pièce ? Votre accueil personnalisé sera effacé.")) return;
    saveDashboard({});
    editing = false;
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

  // ---- Ce que font les boutons ----

  /** Le comportement par défaut, quand la carte n'en a pas choisi. */
  function tapOf(card) {
    if (card.tap) return card.tap;
    return ['light', 'switch', 'group', 'scene'].includes(domainOf(card.entity_id)) ? 'toggle' : 'details';
  }

  function holdOf(card) {
    if (card.hold) return card.hold;
    return domainOf(card.entity_id) === 'automation' && card.facet !== 'enabled' ? 'rearm' : 'details';
  }

  function openDetails(c) {
    const d = domainOf(c.entity?.entity_id);
    if (c.device && ['light', 'switch', 'sensor', 'binary_sensor', 'siren'].includes(d)) {
      sheet = { kind: 'device', device: c.device, entity: c.entity };
    } else if (c.entity) {
      sheet = { kind: 'info', entityId: c.entity.entity_id };
    }
  }

  async function runAction(action, c) {
    const r = resolveAction(action, c.card, c.entity);
    if (r.kind === 'details') return openDetails(c);
    if (r.kind === 'command') {
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
      d.sections[sectionIndex].cards.push(card);
      pos = d.sections[sectionIndex].cards.length - 1;
    });
    // Tout de suite ses réglages : nom, appui long, règles…
    const entity = home.entities[card.entity_id];
    sheet = { kind: 'card', ref: { card, entity, device: index.get(card.entity_id) ?? null, section: sectionIndex, index: pos } };
  }

  // ---- Modifier l'accueil : appui long pour attraper une carte, sinon on fait défiler ----
  let drag = $state(null);
  let hold = null;
  const HOLD_DRAG_MS = 350;

  $effect(() => {
    // Pendant qu'une carte suit le doigt, la page ne défile pas.
    const stop = (e) => drag && e.cancelable && e.preventDefault();
    document.addEventListener('touchmove', stop, { passive: false });
    return () => document.removeEventListener('touchmove', stop);
  });

  function slotDown(e, c) {
    if (!editing || e.button > 0) return;
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

  /** Un capteur avec ses propres réglages (règles, nom…) devient un bouton. */
  function asButton(c) {
    const d = domainOf(c.entity?.entity_id);
    if (['scene', 'automation', 'group', 'person'].includes(d)) return true;
    if (d === 'light' || d === 'switch') return false;
    if (!c.device) return true;
    return !!(c.card.rules?.length || c.card.icon || (c.card.tap && c.card.tap !== 'details'));
  }
</script>

{#if home.security && !editing && (isConfigured(home.security.config) || alarmView(home.security.status, home.now).tone !== 'off')}
  <AlarmCard />
{/if}

{#if fired.length && !editing}
  <section class="fired" aria-label="À voir">
    {#each fired as f (f.entity_id)}
      <button
        class="flag"
        use:press={{ onhold: () => runAction('rearm', { card: { entity_id: f.entity_id, facet: 'state' }, entity: f }) }}
        onclick={() => (sheet = { kind: 'info', entityId: f.entity_id })}
      >
        <Icon name="alert" size={18} />
        <span><strong>{f.name}</strong> · depuis {sinceText(f.state.attributes.fired_at || f.state.last_changed, home.now)}</span>
        <em>appui long : réarmer</em>
      </button>
    {/each}
  </section>
{/if}

<section class="hero">
  {#if people.length}
    <p class="people" aria-label="Qui est à la maison">
      {#each people as p (p.entity_id)}
        <span class="person" class:home={p.state?.value === 'home'} title={p.state?.value === 'home' ? 'À la maison' : 'Absent'}>{p.name}</span>
      {/each}
      {#if !atHome.length && people.every((p) => p.state?.value === 'away')}<span class="nobody">Personne à la maison</span>{/if}
    </p>
  {/if}
  <h1>{headline}</h1>
  {#if summary.lightsOn.length}
    {#if litRooms.length}<p>{listNames(litRooms)}.</p>{/if}
    <button class="btn" onclick={() => summary.lightsOn.forEach((e) => command(e.entity_id, 'turn_off'))}>
      Éteindre les lumières
    </button>
  {:else if summary.plugsOn.length}
    <p>{summary.plugsOn.length === 1 ? 'Une prise est en marche.' : `${summary.plugsOn.length} prises sont en marche.`}</p>
  {/if}
</section>

{#if empty && !zigbeeConfigured}
  <section class="setup">
    <h2>Connectez vos appareils Zigbee</h2>
    <p>Heim lit vos appareils via Zigbee2MQTT. Indiquez l'adresse de la machine où tourne le broker MQTT (souvent celle de Home Assistant).</p>
    <ConnectionForm />
  </section>
{/if}

{#if scenes.length && !editing}
  <nav class="scenes" aria-label="Scènes">
    {#each scenes as s (s.entity_id)}
      {@const active = isOn(s)}
      <button class="scene" class:active style:--tint={s.capabilities?.color || null} aria-pressed={active} onclick={() => command(s.entity_id, active ? 'turn_off' : 'turn_on')}>
        <Icon name={s.capabilities?.icon || 'scene'} size={17} />
        {s.name}
        {#if s.state?.attributes?.partial}<span class="partial">en partie</span>{/if}
      </button>
    {/each}
  </nav>
{/if}

{#if running.length && !editing}
  <section class="running" aria-label="Minuteurs en cours">
    {#each running as t (t.entity_id)}<TimerTile entity={t} compact />{/each}
  </section>
{/if}

{#if home.loaded && !empty}
  <div class="bar">
    {#if editing}
      <button class="btn" onclick={() => (editing = false)}><Icon name="check" size={18} /> Terminé</button>
      <button class="btn quiet" onclick={() => (sheet = { kind: 'add' })}><Icon name="plus" size={18} /> Ajouter une carte</button>
      <button class="btn quiet" onclick={() => mutate((d) => d.sections.push({ title: 'Nouvelle section', cards: [] }))}>Nouvelle section</button>
      <button class="btn danger" onclick={resetToAuto}>Rangement automatique</button>
    {:else}
      {#if isAdmin()}<button class="link" onclick={startEditing}><Icon name="pencil" size={16} /> Personnaliser l'accueil</button>{/if}
    {/if}
  </div>
{/if}

{#if editing}
  <p class="hint">Appui long sur une carte pour la déplacer. Touchez-la pour ses réglages : nom, icône, appui court et long, couleurs selon la valeur.</p>
{/if}

<div class="rooms" class:editing>
  {#each sections as sec, s (s + sec.title)}
    {@const lights = sectionLights(sec)}
    <section class="room" class:unsorted={sec.unsorted} data-section={s}>
      <header>
        {#if editing}
          <input
            class="title-input"
            value={sec.title}
            aria-label="Nom de la section"
            onchange={(e) => mutate((d) => (d.sections[s].title = e.currentTarget.value.trim() || 'Section'))}
          />
          <div class="sec-tools">
            <button class="mini" onclick={() => moveSection(s, -1)} disabled={s === 0} aria-label="Monter la section">↑</button>
            <button class="mini" onclick={() => moveSection(s, 1)} disabled={s === sections.length - 1} aria-label="Descendre la section">↓</button>
            <button
              class="mini"
              aria-label="Supprimer la section"
              onclick={() => confirm(`Supprimer la section « ${sec.title} » ? Les appareils restent dans Appareils.`) && mutate((d) => d.sections.splice(s, 1))}
            ><Icon name="close" size={16} /></button>
          </div>
        {:else}
          <h2>
            {#if !sec.unsorted && !custom}<span class="room-ic"><Icon name={roomIcon(home.layout?.rooms?.find((r) => r.name === sec.title) ?? { name: sec.title })} size={20} /></span>{/if}
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
        {/if}
      </header>

      <div class="grid">
        {#each sec.cards as c, i (c.card.entity_id + i)}
          {@const dragging = drag && drag.moved && drag.from.section === s && drag.from.index === i}
          {@const dropBefore = drag?.moved && drag.to?.section === s && drag.to?.index === i}
          {@const button = c.entity && asButton(c)}
          {@const look = c.entity ? appearance(c.card, c.entity, home.now) : null}
          <div
            class="slot"
            class:wide={c.card.size === 2 || (!button && ['sensor', 'binary_sensor'].includes(c.entity?.domain))}
            class:dragging
            class:drop={dropBefore}
            class:lifted={hold && !drag && editing}
            data-slot="{s}:{i}"
            onpointerdown={(e) => slotDown(e, { ...c, section: s, index: i })}
            onpointermove={slotMove}
            onpointerup={() => slotUp({ ...c, section: s, index: i })}
            onpointercancel={slotCancel}
            use:press={{ disabled: editing || !c.entity, onhold: () => runAction(holdOf(c.card), c) }}
            style:transform={dragging ? `translate(${drag.x - drag.startX}px, ${drag.y - drag.startY}px) scale(1.03)` : null}
          >
            {#if !c.entity}
              <div class="ghost">Introuvable (supprimé ?)</div>
            {:else if c.entity.domain === 'light' || c.entity.domain === 'switch'}
              <ControlTile
                entity={c.entity}
                device={c.device}
                wide={c.card.size === 2}
                forMin={c.card.for_min ?? null}
                label={c.card.name || null}
                iconName={c.card.icon || null}
                ontap={tapOf(c.card) === 'toggle' ? null : () => runAction(tapOf(c.card), c)}
                onedit={editing || !isAdmin() ? null : () => (sheet = { kind: 'device', device: c.device, entity: c.entity })}
                onfor={editing ? null : () => (sheet = { kind: 'for', entity: c.entity })}
              />
            {:else if button}
              <ButtonTile card={c.card} entity={c.entity} wide={c.card.size === 2} ontap={() => !editing && runAction(tapOf(c.card), c)} />
            {:else if c.device}
              <SensorCard
                device={c.device}
                tint={look?.color}
                label={c.card.name || null}
                onedit={() => !editing && runAction(tapOf(c.card), c)}
              />
            {/if}
            {#if editing}<span class="veil" aria-hidden="true"></span>{/if}
          </div>
        {/each}
        {#if editing && !sec.cards.length}
          <button class="empty-slot" onclick={() => (sheet = { kind: 'add', section: s })}>Ajouter une carte ici</button>
        {/if}
      </div>

      {#if sec.timers?.length && !editing}
        <div class="timers">
          {#each sec.timers as t (t.entity_id)}<TimerTile entity={t} compact />{/each}
        </div>
      {/if}
    </section>
  {/each}
</div>

{#if sheet?.kind === 'device'}
  <DeviceSheet device={sheet.device} entity={sheet.entity} onclose={() => (sheet = null)} />
{:else if sheet?.kind === 'info'}
  <InfoSheet entityId={sheet.entityId} onclose={() => (sheet = null)} onopen={onnavigate ? (id) => ((sheet = null), onnavigate(`automatismes/${id}`)) : null} />
{:else if sheet?.kind === 'for'}
  <ForSheet entity={sheet.entity} onclose={() => (sheet = null)} />
{:else if sheet?.kind === 'card'}
  <CardSheet
    ref={sheet.ref}
    sections={custom?.sections ?? []}
    onchange={(fn) => mutate(fn)}
    onclose={() => (sheet = null)}
  />
{:else if sheet?.kind === 'add'}
  <AddCardSheet
    dashboard={custom}
    section={sheet.section ?? 0}
    onadd={addAndEdit}
    onclose={() => (sheet = null)}
  />
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

  .people {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 0 0 10px;
  }

  .person {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 10px 3px 8px;
    border-radius: var(--radius-chip);
    background: var(--surface-2);
    color: var(--muted);
    font-size: 14px;
    font-weight: 600;
  }

  .person::before {
    content: '';
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--line);
  }

  .person.home {
    color: var(--ink);
  }

  .person.home::before {
    background: var(--moss);
  }

  .nobody {
    font-size: 14px;
    color: var(--muted);
    align-self: center;
  }

  .hero {
    padding: 8px 0 24px;
  }

  h1 {
    margin: 0;
    font-size: clamp(34px, 9vw, 52px);
    line-height: 1.02;
    font-weight: 600;
    letter-spacing: -0.02em;
    max-width: 14ch;
  }

  .hero p {
    margin: 10px 0 0;
    font-size: 18px;
    color: var(--muted);
  }

  .hero .btn {
    margin-top: 18px;
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

  .scenes {
    display: flex;
    gap: 8px;
    overflow-x: auto;
    padding: 2px 2px 14px;
    margin: 0 -2px 8px;
    scrollbar-width: none;
  }

  .scene {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 18px;
    border-radius: var(--radius-chip);
    border: 1.5px solid var(--line);
    font-weight: 600;
    white-space: nowrap;
  }

  .scene.active {
    background: var(--tint, var(--ember));
    border-color: var(--tint, var(--ember));
    color: var(--ember-ink);
  }

  .scene.active[style*='--tint'] {
    color: #fff;
  }

  .partial {
    font-size: 12px;
    font-weight: 400;
  }

  .running {
    display: grid;
    gap: 10px;
    margin-bottom: 12px;
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

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(150px, 45%), 1fr));
    gap: 10px;
  }

  .slot {
    position: relative;
    min-width: 0;
  }

  .slot.wide {
    grid-column: span 2;
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

  .fired {
    display: grid;
    gap: 8px;
    margin: 4px 0 16px;
  }

  .flag {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    border-radius: 18px;
    background: color-mix(in srgb, #e5484d 16%, var(--surface));
    box-shadow: var(--shadow), inset 0 0 0 1.5px color-mix(in srgb, #e5484d 50%, transparent);
    color: color-mix(in srgb, #e5484d 75%, var(--ink));
    text-align: left;
    -webkit-touch-callout: none;
    user-select: none;
    -webkit-user-select: none;
  }

  .flag span {
    flex: 1;
    color: var(--ink);
  }

  .flag em {
    font-style: normal;
    font-size: 12px;
    color: var(--muted);
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
    min-height: 80px;
    border-radius: var(--radius-tile);
    border: 1.5px dashed var(--line);
    color: var(--muted);
    font-size: 14px;
  }

  .empty-slot {
    grid-column: 1 / -1;
  }

  .scene-card {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 64px;
    padding: 0 16px;
    border-radius: var(--radius-tile);
    border: 1.5px solid var(--line);
    font-weight: 600;
    text-align: left;
  }

  .scene-card.active {
    background: var(--ember);
    border-color: var(--ember);
    color: var(--ember-ink);
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

  .timers {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(260px, 100%), 1fr));
    gap: 8px;
    margin-top: 10px;
  }
</style>
