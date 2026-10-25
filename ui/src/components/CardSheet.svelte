<script>
  // Réglages d'un bouton de l'accueil : nom, icône, taille, ce que font l'appui court et
  // l'appui long, ses règles d'apparence (couleur, icône, texte selon la valeur), sa section.
  import { untrack } from 'svelte';
  import Sheet from './Sheet.svelte';
  import Icon from './Icon.svelte';
  import ButtonTile from './ButtonTile.svelte';
  import {
    COLORS,
    COLOR_NAMES,
    OPS,
    TILE_ICONS,
    actionLabel,
    actionsFor,
    defaultIcon,
    defaultRules,
    domainOf,
    valueText,
  } from '../lib/tiles.js';

  let { ref, sections, onchange, onclose } = $props();

  const s = untrack(() => ref.section);
  const i = untrack(() => ref.index);
  const original = untrack(() => $state.snapshot(ref.card));
  const entity = untrack(() => ref.entity);
  const d = domainOf(original.entity_id);
  const controllable = ['light', 'switch'].includes(d);

  let card = $state(structuredClone(original));
  let target = $state(s);
  let iconPicker = $state(null);
  if (d === 'automation' && !card.facet) card.facet = 'state';

  const actions = $derived(actionsFor(card.entity_id, card.facet));
  const tapDefault = $derived(['light', 'switch', 'group', 'scene'].includes(d) ? 'toggle' : 'details');
  const holdDefault = $derived(d === 'automation' && card.facet !== 'enabled' ? 'rearm' : 'details');
  const suggested = $derived(defaultRules(card, entity));
  const valueNow = $derived(entity ? valueText(card, entity) : '–');
  const binary = $derived(
    d === 'automation' || d === 'scene' || d === 'group' || d === 'binary_sensor' || d === 'light' || d === 'switch' || d === 'person',
  );
  const WORDS = $derived(
    d === 'person'
      ? ['home', 'away']
      : d === 'automation' || d === 'scene'
        ? ['vrai', 'faux']
        : d === 'binary_sensor'
          ? ['on', 'off']
          : ['allumé', 'éteint'],
  );
  const wordLabel = (w) =>
    ({ on: 'Actif / ouvert', off: 'Inactif / fermé', home: 'À la maison', away: 'Absent', vrai: 'Vrai', faux: 'Faux', allumé: 'Allumé', éteint: 'Éteint' })[w] ?? w;

  function addRule() {
    card.rules = [...(card.rules ?? []), binary ? { op: 'eq', value: WORDS[0], color: 'red' } : { op: 'gt', value: '', color: 'orange' }];
  }

  function moveRule(k, delta) {
    const r = card.rules;
    const j = k + delta;
    if (j < 0 || j >= r.length) return;
    [r[k], r[j]] = [r[j], r[k]];
  }

  function clean(c) {
    const out = { ...c };
    for (const k of ['name', 'icon']) if (!out[k]?.trim?.()) delete out[k];
    if (!controllable || !(out.for_min > 0)) delete out.for_min;
    if (out.tap === tapDefault) delete out.tap;
    if (out.hold === holdDefault) delete out.hold;
    out.rules = (out.rules ?? []).filter((r) => String(r.value).trim() !== '');
    if (!out.rules.length) delete out.rules;
    return out;
  }

  function apply() {
    const next = clean($state.snapshot(card));
    onchange((dash) => {
      dash.sections[s].cards[i] = next;
      if (target !== s) {
        dash.sections[s].cards.splice(i, 1);
        dash.sections[target].cards.push(next);
      }
    });
    onclose();
  }

  function shift(delta) {
    onchange((dash) => {
      const cards = dash.sections[s].cards;
      const j = i + delta;
      if (j < 0 || j >= cards.length) return;
      [cards[i], cards[j]] = [cards[j], cards[i]];
    });
    onclose();
  }

  function duplicate() {
    onchange((dash) => dash.sections[s].cards.splice(i + 1, 0, structuredClone(clean($state.snapshot(card)))));
    onclose();
  }

  function remove() {
    onchange((dash) => dash.sections[s].cards.splice(i, 1));
    onclose();
  }
</script>

<Sheet title={card.name || entity?.name || 'Bouton'} {onclose}>
  <div class="preview" aria-label="Aperçu">
    {#if entity}
      <ButtonTile card={clean($state.snapshot(card))} {entity} wide={card.size === 2} ontap={() => {}} />
    {/if}
    <p class="muted">Valeur actuelle : <strong>{valueNow}</strong></p>
  </div>

  <details open>
    <summary>Apparence</summary>
    <label class="field">
      <span>Nom sur le bouton</span>
      <input class="input" bind:value={card.name} placeholder={entity?.name} />
    </label>
    <div class="field">
      <span>Icône</span>
      <div class="icons">
        <button class="ic" class:sel={!card.icon} onclick={() => (card.icon = '')} title="Celle de l'appareil">
          <Icon name={defaultIcon(entity)} size={20} />
        </button>
        {#each TILE_ICONS as ic (ic)}
          <button class="ic" class:sel={card.icon === ic} onclick={() => (card.icon = ic)} aria-label={ic}><Icon name={ic} size={20} /></button>
        {/each}
      </div>
    </div>
    <div class="field">
      <span>Taille</span>
      <div class="seg">
        <button class:on={(card.size ?? 1) === 1} onclick={() => (card.size = 1)}>1 case</button>
        <button class:on={card.size === 2} onclick={() => (card.size = 2)}>2 cases</button>
      </div>
    </div>
  </details>

  <details open>
    <summary>Ce que fait le bouton</summary>
    {#if d === 'automation'}
      <div class="field">
        <span>Il montre</span>
        <div class="seg">
          <button class:on={card.facet !== 'enabled'} onclick={() => (card.facet = 'state')}>Son état (vrai / faux)</button>
          <button class:on={card.facet === 'enabled'} onclick={() => (card.facet = 'enabled')}>S'il est activé</button>
        </div>
      </div>
    {/if}
    <label class="field">
      <span>Appui court</span>
      <select class="input" value={card.tap ?? tapDefault} onchange={(e) => (card.tap = e.currentTarget.value)}>
        {#each actions as a (a)}<option value={a}>{actionLabel(a, card.entity_id, card.facet)}</option>{/each}
      </select>
    </label>
    <label class="field">
      <span>Appui long</span>
      <select class="input" value={card.hold ?? holdDefault} onchange={(e) => (card.hold = e.currentTarget.value)}>
        {#each actions as a (a)}<option value={a}>{actionLabel(a, card.entity_id, card.facet)}</option>{/each}
      </select>
    </label>
    {#if controllable}
      <div class="field">
        <span>Un appui allume pour…</span>
        <div class="seg wrap">
          {#each [0, 5, 15, 30, 60] as m (m)}
            <button class:on={(card.for_min ?? 0) === m} onclick={() => (card.for_min = m)}>{m === 0 ? 'Sans limite' : m < 60 ? `${m} min` : '1 h'}</button>
          {/each}
        </div>
      </div>
    {/if}
  </details>

  <details open>
    <summary>Règles d'apparence</summary>
    <p class="muted small">
      La première règle vraie donne au bouton sa couleur, son icône et son texte.
      Dans le texte : <code>{'{valeur}'}</code>, <code>{'{depuis}'}</code>, <code>{'{nom}'}</code>.
      {#if !card.rules?.length && suggested.length}Sans règle, Heim applique les siennes (ex. : {suggested.map((r) => `${OPS.find((o) => o.id === r.op)?.label} ${r.value} → ${COLOR_NAMES[r.color]?.toLowerCase()}`).join(', ')}).{/if}
    </p>
    {#each card.rules ?? [] as rule, k (k)}
      <div class="rule">
        <div class="when">
          <span class="si">Si</span>
          {#if binary}
            <select class="input" bind:value={rule.op}>
              <option value="eq">est</option>
              <option value="ne">n'est pas</option>
            </select>
            <select class="input" bind:value={rule.value}>
              {#each WORDS as w (w)}<option value={w}>{wordLabel(w)}</option>{/each}
            </select>
          {:else}
            <select class="input op" bind:value={rule.op}>
              {#each OPS as o (o.id)}<option value={o.id}>{o.label}</option>{/each}
            </select>
            <input class="input" bind:value={rule.value} placeholder="25" inputmode="decimal" aria-label="Valeur" />
          {/if}
        </div>
        <div class="colors" role="radiogroup" aria-label="Couleur">
          <button class="sw none" class:sel={!rule.color} onclick={() => (rule.color = '')} title="Sans couleur" aria-label="Sans couleur"></button>
          {#each Object.entries(COLORS) as [id, hex] (id)}
            <button class="sw" class:sel={rule.color === id} style:background={hex} onclick={() => (rule.color = id)} title={COLOR_NAMES[id]} aria-label={COLOR_NAMES[id]}></button>
          {/each}
        </div>
        <div class="then">
          <button class="ic small" onclick={() => (iconPicker = iconPicker === k ? null : k)} aria-label="Icône de la règle">
            <Icon name={rule.icon || card.icon || defaultIcon(entity)} size={18} />
          </button>
          <input class="input" bind:value={rule.text} placeholder="Texte (facultatif) : Courrier !" />
          <button class="mini" onclick={() => moveRule(k, -1)} disabled={k === 0} aria-label="Monter">↑</button>
          <button class="mini" onclick={() => card.rules.splice(k, 1)} aria-label="Supprimer la règle"><Icon name="close" size={14} /></button>
        </div>
        {#if iconPicker === k}
          <div class="icons">
            <button class="ic" class:sel={!rule.icon} onclick={() => ((rule.icon = ''), (iconPicker = null))}>–</button>
            {#each TILE_ICONS as ic (ic)}
              <button class="ic" class:sel={rule.icon === ic} onclick={() => ((rule.icon = ic), (iconPicker = null))} aria-label={ic}><Icon name={ic} size={18} /></button>
            {/each}
          </div>
        {/if}
      </div>
    {/each}
    <button class="btn quiet" onclick={addRule}><Icon name="plus" size={16} /> Ajouter une règle</button>
  </details>

  {#if sections.length > 1}
    <label class="field">
      <span>Section</span>
      <select class="input" bind:value={target}>
        {#each sections as sec, k (k)}<option value={k}>{sec.title}</option>{/each}
      </select>
    </label>
  {/if}

  <div class="actions">
    <button class="btn" onclick={apply}>Enregistrer</button>
    <button class="btn quiet" onclick={() => shift(-1)}>Avant</button>
    <button class="btn quiet" onclick={() => shift(1)}>Après</button>
    <button class="btn quiet" onclick={duplicate}>Dupliquer</button>
    <button class="btn danger" onclick={remove}>Retirer</button>
  </div>
</Sheet>

<style>
  .preview {
    display: grid;
    gap: 6px;
    max-width: 320px;
    margin-bottom: 10px;
  }

  .preview p {
    margin: 0;
    font-size: 14px;
  }

  details {
    border-top: 1px solid var(--line);
    padding: 10px 0;
  }

  summary {
    cursor: pointer;
    font-weight: 700;
    font-size: 17px;
    padding: 4px 0 8px;
  }

  .small {
    font-size: 13px;
    margin: 0 0 10px;
  }

  code {
    font-size: 12px;
    padding: 1px 4px;
    border-radius: 4px;
    background: var(--surface-2);
  }

  .seg {
    display: flex;
    gap: 6px;
  }

  .seg.wrap {
    flex-wrap: wrap;
  }

  .seg button {
    min-height: 42px;
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

  .icons {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .ic {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: 12px;
    background: var(--surface-2);
    color: var(--ink);
  }

  .ic.small {
    flex: none;
  }

  .ic.sel {
    background: var(--ink);
    color: var(--bg);
  }

  .rule {
    display: grid;
    gap: 8px;
    padding: 12px;
    margin-bottom: 10px;
    border-radius: 16px;
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .when,
  .then {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .when .input,
  .then .input {
    min-width: 0;
    flex: 1;
  }

  .when .op {
    flex: 0 0 90px;
  }

  .si {
    font-weight: 700;
    color: var(--muted);
  }

  .colors {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .sw {
    width: 30px;
    height: 30px;
    border-radius: 50%;
    border: 2px solid transparent;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.12);
  }

  .sw.none {
    background: repeating-linear-gradient(45deg, var(--surface-2) 0 4px, var(--line) 4px 6px);
  }

  .sw.sel {
    border-color: var(--ink);
    transform: scale(1.08);
  }

  .mini {
    flex: none;
    width: 34px;
    height: 34px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--muted);
  }

  .mini:disabled {
    opacity: 0.35;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 12px;
  }
</style>
