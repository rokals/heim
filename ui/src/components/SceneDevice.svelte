<script>
  // Un appareil dans une scène : allumé ou éteint, et pour une lampe les réglages qu'on veut
  // fixer (case cochée) — luminosité, blanc, couleur, effet. Ce qui n'est pas coché ne bouge pas.
  import Icon from './Icon.svelte';
  import { SWATCHES } from '../lib/controls.js';
  import { dimmable, summary, tempRange, toKelvin, toMired } from '../lib/scenes.js';

  /** light : contrôles « lumière » de l'appareil (couleur, effets), quand on les connaît. */
  let { entity, target = $bindable(), light = null, open = false, ontoggle, onchange, onremove } = $props();

  const d = $derived(entity?.domain);
  const canDim = $derived(dimmable(entity) || (d === 'group' && entity?.capabilities?.brightness));
  const range = $derived(tempRange(entity));
  const hasColor = $derived(d === 'light' && Boolean(light?.color));
  const effects = $derived(
    d === 'light' && light?.effect?.values
      ? light.effect.values.filter((v) => !String(v).startsWith('stop') && v !== 'finish_effect')
      : [],
  );
  const pct = $derived(target.brightness != null ? Math.max(1, Math.round((target.brightness / 255) * 100)) : 70);
  const swatch = $derived(target.on && target.color ? target.color : null);

  function change(patch) {
    target = { ...target, ...patch };
    onchange?.(target);
  }

  function check(key, on) {
    if (key === 'brightness') change({ brightness: on ? Math.round(0.7 * 255) : null });
    if (key === 'color_temp') change({ color_temp: on ? Math.round((range[0] + range[1]) / 2) : null, ...(on ? { color: null } : {}) });
    if (key === 'color') change({ color: on ? '#ff8a3d' : null, ...(on ? { color_temp: null } : {}) });
    if (key === 'effect') change({ effect: on ? effects[0] : null });
  }

  // Le blanc : du plus chaud (gauche) au plus froid (droite), comme sur la fiche de la lampe.
  const warmGradient = 'linear-gradient(90deg, #ffb46b, #fff1d6 55%, #d6e6ff)';
</script>

<li class="dev" class:open class:off={!target.on}>
  <button class="head" onclick={ontoggle} aria-expanded={open}>
    <span class="ic" class:lit={target.on} style:--c={swatch}>
      <Icon name={d === 'switch' ? 'plug' : 'bulb'} size={20} />
    </span>
    <span class="txt">
      <strong>{entity?.name ?? 'Appareil supprimé'}</strong>
      <span>{entity?.room ? `${entity.room} · ` : ''}{summary(target, entity)}</span>
    </span>
    <span class="chev" aria-hidden="true"><Icon name="chevron" size={18} /></span>
  </button>

  {#if open}
    <div class="body">
      <div class="seg" role="radiogroup" aria-label="Allumé ou éteint">
        <button role="radio" aria-checked={target.on} class:sel={target.on} onclick={() => change({ on: true })}>Allumé</button>
        <button role="radio" aria-checked={!target.on} class:sel={!target.on} onclick={() => change({ on: false })}>Éteint</button>
      </div>

      {#if target.on && d !== 'switch'}
        {#if canDim}
          <div class="row">
            <label class="check"><input type="checkbox" checked={target.brightness != null} onchange={(e) => check('brightness', e.currentTarget.checked)} /> Luminosité</label>
            {#if target.brightness != null}<span class="num val">{pct} %</span>{/if}
          </div>
          {#if target.brightness != null}
            <input
              class="range bright"
              type="range"
              min="1"
              max="100"
              value={pct}
              oninput={(e) => {
                target = { ...target, brightness: Math.round((Number(e.currentTarget.value) / 100) * 255) };
                onchange?.(target);
              }}
              aria-label="Luminosité de {entity?.name}"
            />
          {/if}
        {/if}

        {#if range}
          <div class="row">
            <label class="check"><input type="checkbox" checked={target.color_temp != null} onchange={(e) => check('color_temp', e.currentTarget.checked)} /> Blanc</label>
            {#if target.color_temp != null}<span class="num val">{toKelvin(target.color_temp).toLocaleString('fr-FR')} K</span>{/if}
          </div>
          {#if target.color_temp != null}
            <!-- Curseur en kelvins inversés : à gauche le chaud (beaucoup de mireds). -->
            <input
              class="range"
              style:background={warmGradient}
              type="range"
              min={range[0]}
              max={range[1]}
              value={range[0] + range[1] - target.color_temp}
              oninput={(e) => {
                target = { ...target, color_temp: range[0] + range[1] - Number(e.currentTarget.value) };
                onchange?.(target);
              }}
              aria-label="Blanc de {entity?.name}"
            />
            <div class="ends"><span>Chaud</span><span>Froid</span></div>
          {/if}
        {/if}

        {#if hasColor}
          <div class="row">
            <label class="check"><input type="checkbox" checked={!!target.color} onchange={(e) => check('color', e.currentTarget.checked)} /> Couleur</label>
          </div>
          {#if target.color}
            <div class="swatches">
              {#each SWATCHES as c (c)}
                <button class="sw" class:sel={target.color?.toLowerCase() === c.toLowerCase()} style:background={c} onclick={() => change({ color: c, color_temp: null })} aria-label="Couleur {c}"></button>
              {/each}
              <label class="sw custom" title="Autre couleur">
                <input type="color" value={target.color} onchange={(e) => change({ color: e.currentTarget.value, color_temp: null })} aria-label="Autre couleur" />
              </label>
            </div>
          {/if}
        {/if}

        {#if effects.length}
          <div class="row">
            <label class="check"><input type="checkbox" checked={!!target.effect} onchange={(e) => check('effect', e.currentTarget.checked)} /> Effet</label>
          </div>
          {#if target.effect}
            <select class="input" value={target.effect} onchange={(e) => change({ effect: e.currentTarget.value })}>
              {#each effects as fx (fx)}<option value={fx}>{fx}</option>{/each}
            </select>
          {/if}
        {/if}

        {#if !canDim && !range && !hasColor && d === 'light'}
          <p class="muted small">Cette lampe ne se règle pas plus finement.</p>
        {/if}
      {/if}

      <button class="btn quiet remove" onclick={onremove}><Icon name="close" size={15} /> Retirer de la scène</button>
    </div>
  {/if}
</li>

<style>
  .dev + .dev {
    border-top: 1px solid var(--line);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 10px 12px;
    text-align: left;
  }

  .ic {
    flex: none;
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: 12px;
    background: var(--surface-2);
    color: var(--muted);
  }

  .ic.lit {
    background: var(--c, color-mix(in srgb, var(--ember) 70%, white));
    color: var(--ember-ink);
  }

  .txt {
    flex: 1;
    display: grid;
    min-width: 0;
  }

  .txt span {
    font-size: 13px;
    color: var(--muted);
  }

  .chev {
    color: var(--muted);
    transition: transform 0.2s ease;
  }

  .open .chev {
    transform: rotate(90deg);
  }

  .body {
    display: grid;
    gap: 10px;
    padding: 2px 14px 14px 64px;
  }

  @media (max-width: 480px) {
    .body {
      padding-left: 14px;
    }
  }

  .seg {
    display: flex;
    padding: 3px;
    border-radius: 12px;
    background: var(--surface-2);
    max-width: 280px;
  }

  .seg button {
    flex: 1;
    min-height: 36px;
    border-radius: 9px;
    font-weight: 600;
    color: var(--muted);
  }

  .seg button.sel {
    background: var(--surface);
    color: var(--ink);
    box-shadow: var(--shadow);
  }

  .row {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 600;
  }

  .check input {
    width: 20px;
    height: 20px;
    accent-color: var(--ink);
  }

  .val {
    color: var(--muted);
    font-size: 14px;
  }

  .range {
    width: 100%;
    height: 28px;
    border-radius: 14px;
    appearance: none;
    -webkit-appearance: none;
    background: linear-gradient(90deg, #3a3a3a, #ffd58a);
  }

  .range::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.35);
  }

  .range::-moz-range-thumb {
    width: 26px;
    height: 26px;
    border: 0;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.35);
  }

  .ends {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
    color: var(--muted);
    margin-top: -4px;
  }

  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .sw {
    width: 34px;
    height: 34px;
    border-radius: 50%;
    border: 2px solid transparent;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.12);
  }

  .sw.sel {
    border-color: var(--ink);
    transform: scale(1.08);
  }

  .custom {
    position: relative;
    overflow: hidden;
    background: conic-gradient(red, yellow, lime, cyan, blue, magenta, red);
    cursor: pointer;
  }

  .custom input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
  }

  .small {
    font-size: 13px;
    margin: 0;
  }

  .remove {
    justify-self: start;
    min-height: 36px;
    font-size: 14px;
  }
</style>
