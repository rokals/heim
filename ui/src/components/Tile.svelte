<script>
  // La carte unique de l'accueil, façon Mushroom : une pastille d'icône, un nom, un état.
  // De 1×1 (l'icône seule, le quart d'une tuile d'avant) à 4×4. Selon la place, elle ajoute
  // un curseur de luminosité, une petite courbe des dernières 24 h, d'autres mesures…
  // L'appui court arrive par ontap ; l'appui long est géré par la page.
  import Icon from './Icon.svelte';
  import { appearance, COLORS, domainOf, sinceText } from '../lib/tiles.js';
  import { cardConfig, inkFor, secondary, sparkPath } from '../lib/layout.js';
  import { sparkOf, wantSpark } from '../lib/sparks.svelte.js';
  import { autoRemaining, isAvailable, isOn } from '../lib/house.js';
  import { clock, formatValue } from '../lib/format.js';
  import { command, home } from '../lib/store.svelte.js';

  /** preview : dans l'éditeur, rien ne part vers les appareils. */
  let { card, entity, device = null, ontap = null, onfor = null, preview = false } = $props();

  const cfg = $derived(cardConfig(card, entity));
  const d = $derived(domainOf(entity?.entity_id ?? card.entity_id));
  const look = $derived(appearance(card, entity, home.now));
  const available = $derived(isAvailable(entity));
  const pending = $derived(!!entity && entity.entity_id in home.pending);
  const controllable = $derived(d === 'light' || d === 'switch');
  const on = $derived(controllable ? isOn(entity) : look.active);
  const brightness = $derived(entity?.state?.attributes?.brightness ?? 255);
  const remaining = $derived(controllable ? autoRemaining(entity.entity_id, home.entities, home.now) : null);
  const power = $derived(device?.readings?.find((r) => r.capabilities?.kind === 'power'));
  const lowBattery = $derived(device?.battery != null && device.battery <= 20);
  const others = $derived(
    ['sensor', 'binary_sensor'].includes(d) && device ? device.readings.filter((r) => r.entity_id !== entity.entity_id).slice(0, 2) : [],
  );
  const numeric = $derived(d === 'sensor' && typeof entity?.state?.value === 'number');

  /** La couleur de la carte quand elle est « allumée » ou qu'une règle colore. */
  const accent = $derived.by(() => {
    if (look.color) return look.color;
    if (card.color) return COLORS[card.color] ?? card.color;
    if (d === 'person') return COLORS.green;
    if (['light', 'switch', 'group', 'scene', 'siren'].includes(d)) return 'var(--ember)';
    return COLORS.blue;
  });
  const lit = $derived(on || !!look.color);
  const ink = $derived(String(accent).startsWith('var(') ? 'var(--ember-ink)' : inkFor(accent));

  let drag = $state(null);
  const pct = $derived(Math.round(((drag ?? (on ? brightness : 0)) / 255) * 100));

  const name = $derived(card.name || entity?.name || card.entity_id);

  const stateText = $derived.by(() => {
    if (look.rule?.text) return look.text;
    if (controllable) {
      if (!available) return 'Injoignable';
      if (pending) return on ? 'Extinction…' : 'Allumage…';
      if (!on) return card.for_min ? `${card.for_min} min d'un clic` : 'Éteint';
      if (remaining != null) return clock(remaining);
      const extra = power && d === 'switch' ? ` · ${formatValue(power)}` : '';
      if (d === 'light' && entity.capabilities?.brightness) return `${drag != null ? Math.round((drag / 255) * 100) : pct} %${extra}`;
      return `Allumé${extra}`;
    }
    if (d === 'group') {
      const n = entity?.state?.attributes?.on_count ?? 0;
      const total = entity?.state?.attributes?.members?.length ?? 0;
      return look.active && total ? `${n} sur ${total}` : look.text;
    }
    if (d === 'automation' && card.facet !== 'enabled' && entity?.state?.attributes?.enabled === false) return `${look.text} · désactivé`;
    return look.text;
  });

  const since = $derived(
    sinceText(
      d === 'automation' ? entity?.state?.attributes?.fired_at || entity?.state?.last_changed : entity?.state?.last_changed,
      home.now,
    ),
  );
  const sub = $derived(secondary(cfg.info, stateText, since));
  const big = $derived(numeric && cfg.layout === 'vertical' && cfg.h >= 2);

  // La courbe : demandée seulement si la carte la montre.
  $effect(() => {
    if (cfg.graph && entity) wantSpark(entity.entity_id);
  });
  const spark = $derived(cfg.graph && entity ? sparkPath(sparkOf(entity.entity_id), 100, 30) : null);

  // Curseur et texte : en 2 de haut, l'icône se met à côté du nom pour laisser la place.
  const row = $derived(cfg.layout === 'horizontal' || (cfg.layout === 'vertical' && (cfg.slider || cfg.graph) && cfg.h < 3));
  const iconSize = $derived(cfg.h >= 3 && !row ? 30 : cfg.w === 1 && cfg.h === 1 ? 22 : 22);

  function tap() {
    if (preview) return;
    ontap?.();
  }

  function slide(e) {
    drag = Number(e.currentTarget.value);
  }

  function commit(e) {
    const value = Number(e.currentTarget.value);
    drag = null;
    if (preview) return;
    if (value <= 2) command(entity.entity_id, 'turn_off');
    else command(entity.entity_id, 'turn_on', { brightness: value });
  }
</script>

<article
  class="tile fill-{cfg.fill} layout-{cfg.layout}"
  class:lit
  class:gone={!available}
  class:row
  class:tall={cfg.h >= 3}
  class:mini={cfg.w === 1 && cfg.h === 1}
  class:has-tool={onfor && controllable && available && !preview && cfg.w >= 2 && cfg.h >= 2}
  style:--accent={accent}
  style:--on-accent={ink}
>
  <button class="hit" onclick={tap} aria-pressed={controllable || d === 'scene' ? on : null} aria-label="{name} : {stateText}" tabindex={preview ? -1 : 0}></button>

  {#if spark}
    <svg class="spark" viewBox="0 0 100 30" preserveAspectRatio="none" aria-hidden="true">
      <path class="area" d={spark.area} />
      <path class="line" d={spark.line} vector-effect="non-scaling-stroke" />
    </svg>
  {/if}

  <div class="body">
    {#if cfg.showIcon}
      <span class="shape" class:spin={pending} class:big={cfg.h >= 3 && !row}>
        <Icon name={look.icon} size={iconSize} />
      </span>
    {/if}
    {#if cfg.layout === 'icon'}
      {#if numeric && card.show_state !== false}<span class="tiny num">{formatValue(entity)}</span>{/if}
    {:else}
      <span class="text">
        {#if big}
          {#if cfg.showName}<span class="name">{name}</span>{/if}
          {#if cfg.showState}<span class="value num" class:huge={cfg.h >= 3}>{look.rule?.text ? look.text : formatValue(entity)}</span>{/if}
          {#if cfg.info === 'last_changed' || cfg.info === 'both'}<span class="state">il y a {since}</span>{/if}
        {:else}
          {#if cfg.showName}<span class="name">{name}</span>{/if}
          {#if cfg.showState && sub}<span class="state num">{sub}</span>{/if}
        {/if}
        {#if others.length && (cfg.w >= 3 || cfg.h >= 2)}
          <span class="others num">{others.map((r) => formatValue(r)).join(' · ')}</span>
        {/if}
      </span>
    {/if}
  </div>

  {#if cfg.slider}
    <div class="slider" style:--pct="{pct}%">
      <input
        type="range"
        min="0"
        max="255"
        value={drag ?? (on ? brightness : 0)}
        disabled={!available || preview}
        oninput={slide}
        onchange={commit}
        aria-label="Luminosité de {name}"
        data-nopress
      />
    </div>
  {/if}

  {#if onfor && controllable && available && !preview && cfg.w >= 2 && cfg.h >= 2}
    <button class="tool" onclick={onfor} aria-label="Allumer {name} pour un temps" title="Allumer pour…" data-nopress>
      <Icon name="timer" size={16} />
    </button>
  {/if}

  {#if lowBattery}
    <span class="battery num" class:dot={cfg.w === 1 || cfg.h === 1} title="Pile faible : {device.battery} %">
      {#if cfg.w > 1 && cfg.h > 1}<Icon name="battery" size={13} />{device.battery} %{/if}
    </span>
  {/if}
</article>

<style>
  .tile {
    --pad: 12px;
    position: relative;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 10px;
    height: 100%;
    padding: var(--pad);
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
    overflow: hidden;
    isolation: isolate;
    -webkit-touch-callout: none;
    user-select: none;
    -webkit-user-select: none;
    transition: background-color 0.35s ease, color 0.35s ease;
  }

  .hit {
    position: absolute;
    inset: 0;
    z-index: 0;
    border-radius: inherit;
  }

  .hit:focus-visible {
    outline: 2px solid var(--ink);
    outline-offset: -3px;
  }

  .body,
  .slider,
  .tool,
  .battery {
    position: relative;
    z-index: 1;
  }

  /* Le contenu laisse passer le doigt jusqu'au bouton, sauf le curseur et l'outil. */
  .body {
    pointer-events: none;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 8px;
    min-width: 0;
    min-height: 0;
  }

  .tile:not(.layout-horizontal) .body {
    flex: 1 1 auto;
    justify-content: center;
  }

  .row .body {
    flex-direction: row;
    text-align: left;
    gap: 12px;
  }

  .layout-icon .body {
    gap: 2px;
  }

  .has-tool.row .body {
    padding-right: 26px;
  }

  .shape {
    flex: none;
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--muted);
    transition: background-color 0.35s ease, color 0.35s ease;
  }

  .shape.big {
    width: 60px;
    height: 60px;
  }

  .mini .shape {
    width: 38px;
    height: 38px;
  }

  .text {
    display: grid;
    gap: 1px;
    min-width: 0;
    max-width: 100%;
  }

  .name {
    font-size: 15px;
    font-weight: 650;
    line-height: 1.2;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .tall .name {
    font-size: 17px;
  }

  .state,
  .others,
  .tiny {
    font-size: 13px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .tiny {
    font-size: 12px;
    font-weight: 600;
  }

  .value {
    font-size: 24px;
    font-weight: 650;
    letter-spacing: -0.01em;
    line-height: 1.1;
  }

  .value.huge {
    font-size: 40px;
  }

  /* ---- Remplissage quand la carte est « allumée » ---- */
  .lit .shape {
    background: var(--accent);
    color: var(--on-accent);
  }

  .fill-tint.lit {
    background: color-mix(in srgb, var(--accent) 16%, var(--surface));
  }

  .fill-none.lit .shape {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    color: var(--accent);
  }

  .fill-solid.lit {
    background: var(--accent);
    color: var(--on-accent);
  }

  .fill-solid.lit::before {
    content: '';
    position: absolute;
    inset: 0;
    z-index: -1;
    background: radial-gradient(140% 110% at 16% 10%, rgba(255, 255, 255, 0.45) 0%, transparent 65%);
  }

  .fill-solid.lit .shape {
    background: color-mix(in srgb, white 45%, transparent);
    color: var(--on-accent);
  }

  .fill-solid.lit .slider {
    background: rgba(0, 0, 0, 0.14);
  }

  .fill-solid.lit .slider::before {
    background: color-mix(in srgb, white 70%, transparent);
  }

  .fill-solid.lit .state,
  .fill-solid.lit .others,
  .fill-solid.lit .tiny {
    color: color-mix(in srgb, var(--on-accent) 78%, transparent);
  }

  .gone {
    opacity: 0.55;
  }

  .spin {
    animation: breathe 1.2s ease-in-out infinite;
  }

  @keyframes breathe {
    50% {
      opacity: 0.45;
    }
  }

  /* ---- Curseur façon Mushroom : une barre qui se remplit ---- */
  .slider {
    height: 40px;
    flex: none;
    border-radius: 12px;
    overflow: hidden;
    background: color-mix(in srgb, var(--accent) 20%, var(--surface-2));
  }

  .slider::before {
    content: '';
    position: absolute;
    inset: 0 auto 0 0;
    width: var(--pct);
    background: var(--accent);
    transition: width 0.15s ease;
    pointer-events: none;
  }

  .slider input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    margin: 0;
    opacity: 0;
    cursor: ew-resize;
    touch-action: pan-y;
  }

  .slider input:disabled {
    cursor: default;
  }

  .slider:has(input:focus-visible) {
    outline: 2px solid var(--ink);
    outline-offset: 2px;
  }

  /* Bandeau d'une case de haut : le curseur prend la droite. */
  .layout-horizontal:has(.slider) {
    flex-direction: row;
    align-items: center;
  }

  .layout-horizontal .body {
    flex: 1 1 auto;
  }

  .layout-horizontal .slider {
    flex: 0 0 45%;
  }

  /* ---- Courbe ---- */
  .spark {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    width: 100%;
    height: 40%;
    z-index: 0;
    pointer-events: none;
    color: var(--accent);
  }

  .spark .area {
    fill: currentColor;
    opacity: 0.14;
  }

  .spark .line {
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    opacity: 0.7;
  }

  /* Bandeau : la courbe à droite, le texte à gauche. */
  .layout-horizontal .spark {
    left: 48%;
    width: 52%;
    height: 60%;
    bottom: 20%;
  }

  .layout-horizontal:has(.spark) .body {
    max-width: 50%;
  }

  .layout-horizontal:has(.spark) {
    justify-content: center;
  }

  .tile:not(.lit) .spark {
    color: var(--muted);
  }

  .tile:not(.layout-horizontal):has(.spark) {
    justify-content: flex-start;
  }

  .tile:not(.layout-horizontal):has(.spark) .body {
    flex: 0 0 auto;
  }

  .tool {
    position: absolute;
    top: 6px;
    right: 6px;
    width: 34px;
    height: 34px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: inherit;
    opacity: 0.6;
  }

  .tool:hover,
  .tool:focus-visible {
    opacity: 1;
    background: color-mix(in srgb, currentColor 12%, transparent);
  }

  .battery {
    position: absolute;
    bottom: 8px;
    right: 10px;
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: 11px;
    font-weight: 700;
    color: var(--alert);
    pointer-events: none;
  }

  .battery.dot {
    top: 8px;
    bottom: auto;
    right: 8px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--alert);
  }
</style>
