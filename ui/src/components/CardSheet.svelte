<script>
  import { tr } from '../lib/i18n.js';
  // Réglages d'une carte de l'accueil, en deux onglets :
  //  - Fonction : ce qu'elle montre (automatisme), appui court, long, double, minuterie, section ;
  //  - Apparence : taille (de 1×1 à 4×4), disposition, nom, icône, couleur, remplissage,
  //    infos affichées, curseur, courbe, et couleurs selon la valeur (règles).
  // L'aperçu en haut est à la vraie taille de la grille.
  import { untrack } from 'svelte';
  import Sheet from './Sheet.svelte';
  import Icon from './Icon.svelte';
  import Tile from './Tile.svelte';
  import HouseSpecial from './HouseSpecial.svelte';
  import { houseActionLabel, houseActions, houseDefaults, kindInfo } from '../lib/housecards.js';
  import { home, homeRooms } from '../lib/store.svelte.js';
  import { isConfigured } from '../lib/security.js';
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
  import { INFOS, LAYOUTS, SIZE_PRESETS, cardSize, cleanAppearance, showGraph, showSlider } from '../lib/layout.js';

  let { ref, sections, onchange, onclose } = $props();

  const s = untrack(() => ref.section);
  const i = untrack(() => ref.index);
  const original = untrack(() => $state.snapshot(ref.card));
  const entity = untrack(() => ref.entity);
  const device = untrack(() => ref.device);
  const kind = original.type ?? 'entity';
  const d = domainOf(original.entity_id);
  const controllable = ['light', 'switch'].includes(d);
  // Cartes « Maison » : sans appareil, elles se règlent comme les autres (sauf Scènes,
  // Minuteurs et Horloge, qui ont leur propre affichage : taille et section seulement).
  const house = kind === 'house';
  const special = house && ['scenes', 'timers', 'clock'].includes(original.kind);
  const tiled = kind === 'entity' || (house && !special);

  let card = $state({ ...structuredClone(original), ...cardSize(original, entity) });
  delete card.size;
  let target = $state(s);
  let tab = $state(tiled ? 'function' : 'look');
  let iconPicker = $state(null);
  let showIcons = $state(false);
  if (d === 'automation' && !card.facet) card.facet = 'state';

  const actions = $derived(house ? houseActions(card.kind) : actionsFor(card.entity_id, card.facet));
  const label = (a) => (house ? houseActionLabel(a, card.kind) : actionLabel(a, card.entity_id, card.facet));
  const tapDefault = $derived(
    house ? houseDefaults(card.kind).tap : ['light', 'switch', 'group', 'scene'].includes(d) ? 'toggle' : 'details',
  );
  const holdDefault = $derived(
    house ? houseDefaults(card.kind).hold : d === 'automation' && card.facet !== 'enabled' ? 'rearm' : 'details',
  );
  const alarmReady = $derived(isConfigured(home.security?.config));
  const suggested = $derived(entity ? defaultRules(card, entity) : []);
  const valueNow = $derived(entity ? valueText(card, entity) : '–');
  const canSlider = $derived(d === 'light' && Boolean(entity?.capabilities?.brightness));
  const canGraph = $derived(d === 'sensor' && typeof entity?.state?.value === 'number');
  const sliderAuto = $derived(showSlider({ ...card, slider: undefined }, entity, card.w, card.h));
  const graphAuto = $derived(showGraph({ ...card, graph: undefined }, entity, card.w, card.h));
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
          : ["allumé", "éteint"],
  );
  const wordLabel = (w) =>
    ({ on: tr("Actif / ouvert"), off: tr("Inactif / fermé"), home: tr("À la maison"), away: tr("Absent"), vrai: tr("Vrai"), faux: tr("Faux"), allumé: tr("Allumé"), éteint: tr("Éteint") })[w] ?? w;

  // Aperçu à la vraie taille : une case = 1/4 de la largeur de l'accueil sur téléphone.
  let previewWidth = $state(0);
  const UNIT_GAP = 10;
  const ROW = 70;
  const unit = $derived(Math.max(56, Math.min(110, (previewWidth - UNIT_GAP * 3) / 4)));

  function setSize(w, h) {
    card.w = w;
    card.h = h;
  }

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
    let out = { ...c };
    for (const k of ['name', 'icon', 'color', 'text', 'subtitle', 'align']) if (!String(out[k] ?? '').trim()) delete out[k];
    if (!tiled) {
      for (const k of ['tap', 'hold', 'double', 'rules', 'for_min', 'facet']) delete out[k];
      return out;
    }
    if (!out.arm) delete out.arm;
    if (!controllable || !(out.for_min > 0)) delete out.for_min;
    if (out.tap === tapDefault) delete out.tap;
    if (out.hold === holdDefault) delete out.hold;
    if (!out.double || out.double === 'none') delete out.double;
    out.rules = (out.rules ?? []).filter((r) => String(r.value).trim() !== '');
    if (!out.rules.length) delete out.rules;
    if (!canSlider) delete out.slider;
    if (!canGraph) delete out.graph;
    out = cleanAppearance(out);
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

  const triState = (v) => (v === true ? 'on' : v === false ? 'off' : 'auto');
  const fromTri = (t) => (t === 'on' ? true : t === 'off' ? false : undefined);
  const title = kind === 'title' ? tr("Titre") : kind === 'spacer' ? tr("Espace vide") : house ? kindInfo(original.kind).name : null;
</script>

<Sheet title={title ?? (card.name || entity?.name || tr("Tuile"))} {onclose}>
  <div class="preview" bind:clientWidth={previewWidth} aria-label={tr("Aperçu")}>
    <div class="stage" style:width="{card.w * unit + (card.w - 1) * UNIT_GAP}px" style:height="{card.h * ROW + (card.h - 1) * UNIT_GAP}px">
      {#if kind === 'title'}
        <div class="title-prev" class:center={card.align === 'center'}>
          <strong>{card.text || tr("Titre")}</strong>
          {#if card.subtitle}<span>{card.subtitle}</span>{/if}
        </div>
      {:else if kind === 'spacer'}
        <div class="spacer-prev">{tr("Espace")}</div>
      {:else if special}
        <HouseSpecial card={clean($state.snapshot(card))} preview />
      {:else if entity}
        <Tile card={clean($state.snapshot(card))} {entity} {device} preview />
      {/if}
    </div>
    {#if entity && !house}<p class="muted">{tr("Valeur actuelle :")} <strong>{valueNow}</strong></p>{/if}
  </div>

  {#if tiled}
    <div class="tabs" role="tablist">
      <button role="tab" aria-selected={tab === 'function'} class:on={tab === 'function'} onclick={() => (tab = 'function')}>
        <Icon name="power" size={16} /> {tr("Fonction")}
      </button>
      <button role="tab" aria-selected={tab === 'look'} class:on={tab === 'look'} onclick={() => (tab = 'look')}>
        <Icon name="star" size={16} /> {tr("Apparence")}
      </button>
    </div>
  {/if}

  {#if tab === 'function'}
    <div class="pane" role="tabpanel">
      {#if d === 'automation'}
        <div class="field">
          <span>{tr("La tuile montre")}</span>
          <div class="seg">
            <button class:on={card.facet !== 'enabled'} onclick={() => (card.facet = 'state')}>{tr("Son état (vrai / faux)")}</button>
            <button class:on={card.facet === 'enabled'} onclick={() => (card.facet = 'enabled')}>{tr("S'il est activé")}</button>
          </div>
        </div>
      {/if}
      <label class="field">
        <span>{tr("Appui court")}</span>
        <select class="input" value={card.tap ?? tapDefault} onchange={(e) => (card.tap = e.currentTarget.value)}>
          {#each actions as a (a)}<option value={a}>{label(a)}</option>{/each}
        </select>
      </label>
      <label class="field">
        <span>{tr("Appui long")}</span>
        <select class="input" value={card.hold ?? holdDefault} onchange={(e) => (card.hold = e.currentTarget.value)}>
          {#each actions as a (a)}<option value={a}>{label(a)}</option>{/each}
        </select>
      </label>
      <label class="field">
        <span>{tr("Double appui")}</span>
        <select class="input" value={card.double ?? 'none'} onchange={(e) => (card.double = e.currentTarget.value)}>
          {#each actions as a (a)}<option value={a}>{a === 'none' ? tr("Rien (l’appui court part tout de suite)") : actionLabel(a, card.entity_id, card.facet)}</option>{/each}
        </select>
      </label>
      {#if house && card.kind === 'room'}
        <label class="field">
          <span>{tr("La pièce")}</span>
          <select class="input" bind:value={card.room}>
            {#each homeRooms() as r (r)}<option value={r}>{r}</option>{/each}
          </select>
        </label>
      {/if}
      {#if house && card.kind === 'leave' && alarmReady}
        <label class="check-row">
          <input type="checkbox" checked={!!card.arm} onchange={(e) => (card.arm = e.currentTarget.checked)} />
          {tr("Activer aussi l'alarme en mode Absent")}
        </label>
      {/if}
      {#if controllable}
        <div class="field">
          <span>{tr("Un appui allume pour…")}</span>
          <div class="seg wrap">
            {#each [0, 5, 15, 30, 60] as m (m)}
              <button class:on={(card.for_min ?? 0) === m} onclick={() => (card.for_min = m)}>{m === 0 ? tr("Sans limite") : m < 60 ? `${m} min` : '1 h'}</button>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  {:else}
    <div class="pane" role="tabpanel">
      {#if kind === 'title'}
        <label class="field">
          <span>{tr("Titre")}</span>
          <input class="input" bind:value={card.text} placeholder={tr("Lumières, Climat, Sécurité…")} />
        </label>
        <label class="field">
          <span>{tr("Sous-titre (facultatif)")}</span>
          <input class="input" bind:value={card.subtitle} />
        </label>
        <div class="field">
          <span>{tr("Alignement")}</span>
          <div class="seg">
            <button class:on={card.align !== 'center'} onclick={() => (card.align = '')}>{tr("À gauche")}</button>
            <button class:on={card.align === 'center'} onclick={() => (card.align = 'center')}>{tr("Centré")}</button>
          </div>
        </div>
      {/if}

      <div class="field">
        <span>{tr("Taille")} <strong class="num">{card.w} × {card.h}</strong></span>
        <div class="size">
          <div class="picker" role="grid" aria-label={tr("Choisir la taille")}>
            {#each [1, 2, 3, 4] as h (h)}
              {#each [1, 2, 3, 4] as w (w)}
                <button
                  class="cell"
                  class:in={w <= card.w && h <= card.h}
                  onclick={() => setSize(w, h)}
                  aria-label={tr("{w} de large, {h} de haut", { w, h })}
                  title="{w} × {h}"
                ></button>
              {/each}
            {/each}
          </div>
          <div class="presets">
            {#each SIZE_PRESETS as p (p.label)}
              <button class:on={card.w === p.w && card.h === p.h} onclick={() => setSize(p.w, p.h)}>
                <span class="glyph" style:aspect-ratio="{p.w * 1.15} / {p.h}"></span>
                {p.label}
              </button>
            {/each}
          </div>
        </div>
      </div>

      {#if tiled}
        <div class="field">
          <span>{tr("Disposition")}</span>
          <div class="seg wrap">
            {#each LAYOUTS as l (l.id)}
              <button class:on={(card.layout ?? 'auto') === l.id} onclick={() => (card.layout = l.id)}>{l.label}</button>
            {/each}
          </div>
        </div>

        <label class="field">
          <span>{tr("Nom")}</span>
          <input class="input" bind:value={card.name} placeholder={entity?.name} />
        </label>

        <div class="toggles">
          <label><input type="checkbox" checked={card.show_name !== false} onchange={(e) => (card.show_name = e.currentTarget.checked)} /> {tr("Nom")}</label>
          <label><input type="checkbox" checked={card.show_state !== false} onchange={(e) => (card.show_state = e.currentTarget.checked)} /> {tr("État")}</label>
          <label><input type="checkbox" checked={card.show_icon !== false} onchange={(e) => (card.show_icon = e.currentTarget.checked)} /> {tr("Icône")}</label>
        </div>

        <div class="field">
          <span>{tr("Sous le nom")}</span>
          <div class="seg wrap">
            {#each INFOS as o (o.id)}
              <button class:on={(card.info ?? 'state') === o.id} onclick={() => (card.info = o.id)}>{o.label}</button>
            {/each}
          </div>
        </div>

        <div class="field">
          <span>{tr("Icône")}</span>
          <div class="icons">
            <button class="ic" class:sel={!card.icon} onclick={() => (card.icon = '')} title={tr("Celle de l'appareil")}>
              <Icon name={defaultIcon(entity)} size={20} />
            </button>
            {#each showIcons ? TILE_ICONS : TILE_ICONS.slice(0, 11) as ic (ic)}
              <button class="ic" class:sel={card.icon === ic} onclick={() => (card.icon = ic)} aria-label={ic}><Icon name={ic} size={20} /></button>
            {/each}
            {#if !showIcons}<button class="ic more" onclick={() => (showIcons = true)}>+{TILE_ICONS.length - 11}</button>{/if}
          </div>
        </div>

        <div class="field">
          <span>{tr("Couleur quand c'est allumé / actif")}</span>
          <div class="colors" role="radiogroup" aria-label={tr("Couleur")}>
            <button class="sw auto" class:sel={!card.color} onclick={() => (card.color = '')} title={tr("Automatique")} aria-label={tr("Automatique")}>A</button>
            {#each Object.entries(COLORS) as [id, hex] (id)}
              <button class="sw" class:sel={card.color === id} style:background={hex} onclick={() => (card.color = id)} title={COLOR_NAMES[id]} aria-label={COLOR_NAMES[id]}></button>
            {/each}
          </div>
        </div>

        <div class="field">
          <span>{tr("Remplissage")}</span>
          <div class="seg">
            <button class:on={(card.fill ?? 'tint') === 'tint'} onclick={() => (card.fill = 'tint')}>{tr("Teinté")}</button>
            <button class:on={card.fill === 'solid'} onclick={() => (card.fill = 'solid')}>{tr("Plein")}</button>
            <button class:on={card.fill === 'none'} onclick={() => (card.fill = 'none')}>{tr("Discret")}</button>
          </div>
        </div>

        {#if canSlider}
          <div class="field">
            <span>{tr("Curseur de luminosité")}</span>
            <div class="seg">
              {#each [['auto', (sliderAuto ? tr("Auto (oui à cette taille)") : tr("Auto (non à cette taille)"))], ['on', tr("Toujours")], ['off', tr("Jamais")]] as [id, label] (id)}
                <button class:on={triState(card.slider) === id} onclick={() => (card.slider = fromTri(id))}>{label}</button>
              {/each}
            </div>
          </div>
        {/if}
        {#if canGraph}
          <div class="field">
            <span>{tr("Courbe des dernières 24 h")}</span>
            <div class="seg">
              {#each [['auto', (graphAuto ? tr("Auto (oui à cette taille)") : tr("Auto (non à cette taille)"))], ['on', tr("Toujours")], ['off', tr("Jamais")]] as [id, label] (id)}
                <button class:on={triState(card.graph) === id} onclick={() => (card.graph = fromTri(id))}>{label}</button>
              {/each}
            </div>
          </div>
        {/if}

        <details open={!!card.rules?.length}>
          <summary>{tr("Couleurs selon la valeur")} {#if card.rules?.length}<span class="count">{card.rules.length}</span>{/if}</summary>
          <p class="muted small">
            {tr("La première règle vraie donne à la tuile sa couleur, son icône et son texte. Dans le texte :")} <code>{'{valeur}'}</code>, <code>{'{depuis}'}</code>, <code>{'{nom}'}</code>.
            {#if !card.rules?.length && suggested.length}{tr("Sans règle, Heim applique les siennes (ex. : {join}).", { join: suggested.map((r) => `${OPS.find((o) => o.id === r.op)?.label} ${r.value} → ${COLOR_NAMES[r.color]?.toLowerCase()}`).join(', ') })}{/if}
          </p>
          {#each card.rules ?? [] as rule, k (k)}
            <div class="rule">
              <div class="when">
                <span class="si">{tr("Si")}</span>
                {#if binary}
                  <select class="input" bind:value={rule.op}>
                    <option value="eq">{tr("est")}</option>
                    <option value="ne">{tr("n'est pas")}</option>
                  </select>
                  <select class="input" bind:value={rule.value}>
                    {#each WORDS as w (w)}<option value={w}>{wordLabel(w)}</option>{/each}
                  </select>
                {:else}
                  <select class="input op" bind:value={rule.op}>
                    {#each OPS as o (o.id)}<option value={o.id}>{o.label}</option>{/each}
                  </select>
                  <input class="input" bind:value={rule.value} placeholder="25" inputmode="decimal" aria-label={tr("Valeur")} />
                {/if}
              </div>
              <div class="colors" role="radiogroup" aria-label={tr("Couleur")}>
                <button class="sw none" class:sel={!rule.color} onclick={() => (rule.color = '')} title={tr("Sans couleur")} aria-label={tr("Sans couleur")}></button>
                {#each Object.entries(COLORS) as [id, hex] (id)}
                  <button class="sw" class:sel={rule.color === id} style:background={hex} onclick={() => (rule.color = id)} title={COLOR_NAMES[id]} aria-label={COLOR_NAMES[id]}></button>
                {/each}
              </div>
              <div class="then">
                <button class="ic small" onclick={() => (iconPicker = iconPicker === k ? null : k)} aria-label={tr("Icône de la règle")}>
                  <Icon name={rule.icon || card.icon || defaultIcon(entity)} size={18} />
                </button>
                <input class="input" bind:value={rule.text} placeholder={tr("Texte (facultatif) : Courrier !")} />
                <button class="mini" onclick={() => moveRule(k, -1)} disabled={k === 0} aria-label={tr("Monter")}>↑</button>
                <button class="mini" onclick={() => card.rules.splice(k, 1)} aria-label={tr("Supprimer la règle")}><Icon name="close" size={14} /></button>
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
          <button class="btn quiet" onclick={addRule}><Icon name="plus" size={16} /> {tr("Ajouter une règle")}</button>
        </details>
      {/if}
    </div>
  {/if}

  {#if sections.length > 1}
    <label class="field">
      <span>{tr("Section")}</span>
      <select class="input" bind:value={target}>
        {#each sections as sec, k (k)}<option value={k}>{sec.title || tr("En haut (sans titre)")}</option>{/each}
      </select>
    </label>
  {/if}

  <div class="actions">
    <button class="btn" onclick={apply}>{tr("Enregistrer")}</button>
    <button class="btn quiet" onclick={() => shift(-1)}>{tr("Avant")}</button>
    <button class="btn quiet" onclick={() => shift(1)}>{tr("Après")}</button>
    <button class="btn quiet" onclick={duplicate}>{tr("Dupliquer")}</button>
    <button class="btn danger" onclick={remove}>{tr("Retirer")}</button>
  </div>
</Sheet>

<style>
  .preview {
    display: grid;
    justify-items: center;
    gap: 8px;
    padding: 16px 0 14px;
    margin-bottom: 6px;
    border-radius: 18px;
    background: repeating-linear-gradient(0deg, transparent 0 79px, color-mix(in srgb, var(--line) 50%, transparent) 79px 80px), var(--surface-2);
  }

  .stage {
    max-width: 100%;
    transition: width 0.2s ease, height 0.2s ease;
  }

  .preview p {
    margin: 0;
    font-size: 14px;
  }

  .title-prev {
    height: 100%;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
  }

  .title-prev.center {
    align-items: center;
  }

  .title-prev strong {
    font-size: 20px;
  }

  .title-prev span {
    color: var(--muted);
    font-size: 14px;
  }

  .spacer-prev {
    height: 100%;
    display: grid;
    place-items: center;
    border-radius: var(--radius-tile);
    border: 1.5px dashed var(--line);
    color: var(--muted);
    font-size: 13px;
  }

  .tabs {
    display: flex;
    padding: 3px;
    border-radius: 14px;
    background: var(--surface-2);
    margin: 4px 0 6px;
  }

  .tabs button {
    flex: 1;
    min-height: 42px;
    border-radius: 11px;
    font-weight: 650;
    color: var(--muted);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
  }

  .tabs button.on {
    background: var(--surface);
    color: var(--ink);
    box-shadow: var(--shadow);
  }

  .pane {
    display: grid;
    gap: 4px;
  }

  .size {
    display: flex;
    gap: 16px;
    align-items: flex-start;
    flex-wrap: wrap;
  }

  .picker {
    display: grid;
    grid-template-columns: repeat(4, 30px);
    grid-auto-rows: 26px;
    gap: 4px;
    padding: 6px;
    border-radius: 12px;
    background: var(--surface-2);
    touch-action: manipulation;
  }

  .cell {
    border-radius: 6px;
    background: var(--surface);
    box-shadow: inset 0 0 0 1px var(--line);
  }

  .cell.in {
    background: var(--ember);
    box-shadow: none;
  }

  .cell:hover {
    box-shadow: inset 0 0 0 2px var(--ink);
  }

  .presets {
    flex: 1;
    min-width: 170px;
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 6px;
  }

  .presets button {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 38px;
    padding: 0 10px;
    border-radius: 12px;
    border: 1.5px solid var(--line);
    font-weight: 600;
    font-size: 14px;
  }

  .presets button.on {
    border-color: var(--ink);
    background: var(--ink);
    color: var(--bg);
  }

  .glyph {
    height: 16px;
    max-width: 30px;
    border-radius: 3px;
    background: currentColor;
    opacity: 0.6;
  }

  .check-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 10px 0;
    font-weight: 600;
  }

  .check-row input {
    width: 20px;
    height: 20px;
    accent-color: var(--ink);
  }

  .toggles {
    display: flex;
    gap: 16px;
    flex-wrap: wrap;
    margin: 4px 0 8px;
  }

  .toggles label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
  }

  .toggles input {
    width: 20px;
    height: 20px;
    accent-color: var(--ink);
  }

  details {
    border-top: 1px solid var(--line);
    padding: 10px 0;
    margin-top: 6px;
  }

  summary {
    cursor: pointer;
    font-weight: 700;
    font-size: 17px;
    padding: 4px 0 8px;
  }

  .count {
    font-size: 12px;
    padding: 1px 7px;
    border-radius: 9px;
    background: var(--surface-2);
    vertical-align: middle;
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

  .ic.more {
    font-size: 13px;
    font-weight: 700;
    color: var(--muted);
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

  .sw.auto {
    background: var(--surface-2);
    font-size: 12px;
    font-weight: 800;
    color: var(--muted);
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
