<script>
  // Le panneau d'une lampe : luminosité, blanc chaud ou froid, couleur, effet, fondu.
  import { command, api, toast } from '../lib/store.svelte.js';
  import { FADES, SWATCHES, hsToHex, kelvin, valueLabel, xyToHex } from '../lib/controls.js';

  let { entity, light = null, values = {}, deviceId = null, canSet = true } = $props();

  const on = $derived(entity.state?.value === 'on');
  const caps = $derived(entity.capabilities ?? {});
  const dimmable = $derived(Boolean(caps.brightness) || Boolean(light?.brightness));
  const ctRange = $derived(Array.isArray(caps.color_temp) ? caps.color_temp : light?.color_temp ? [light.color_temp.min ?? 153, light.color_temp.max ?? 500] : null);
  const hasColor = $derived(Boolean(light?.color) && canSet);
  const effects = $derived(canSet && light?.effect?.values ? light.effect.values.filter((v) => !String(v).startsWith('stop') && v !== 'finish_effect') : []);
  const advanced = $derived(Boolean(deviceId) && canSet && Boolean(light));

  let bright = $state(null);
  let temp = $state(null);
  const shownBright = $derived(bright ?? entity.state?.attributes?.brightness ?? 254);
  const shownTemp = $derived(temp ?? entity.state?.attributes?.color_temp ?? ctRange?.[0] ?? 300);
  const currentColor = $derived(values.color ? (values.color.x != null ? xyToHex(values.color) : hsToHex(values.color)) : null);
  const colorMode = $derived(values.color_mode);

  const fadeKey = $derived(`heim_fade_${deviceId}`);
  let fade = $state(0);
  $effect(() => {
    try {
      fade = Number(localStorage.getItem(fadeKey) ?? 0);
    } catch {
      fade = 0;
    }
  });
  function setFade(v) {
    fade = v;
    try {
      localStorage.setItem(fadeKey, String(v));
    } catch {
      /* navigation privée */
    }
  }

  async function send(values) {
    try {
      await api(`/devices/${encodeURIComponent(deviceId)}/set`, { method: 'POST', body: { values } });
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  function withFade(v) {
    return advanced && fade > 0 ? { ...v, transition: fade } : null;
  }

  function toggle() {
    const v = withFade({ state: on ? 'OFF' : 'ON' });
    if (v) send(v);
    else command(entity.entity_id, on ? 'turn_off' : 'turn_on');
  }

  function setBrightness(b) {
    bright = null;
    const v = withFade({ state: 'ON', brightness: b });
    if (v) send(v);
    else command(entity.entity_id, 'turn_on', { brightness: b });
  }

  function setTemp(t) {
    temp = null;
    const v = withFade({ state: 'ON', color_temp: t });
    if (v) send(v);
    else command(entity.entity_id, 'turn_on', { color_temp: t });
  }

  function setColor(hex) {
    send({ state: 'ON', color: { hex }, ...(fade > 0 ? { transition: fade } : {}) });
  }

  const pct = $derived(Math.max(1, Math.round((shownBright / 255) * 100)));
</script>

<section class="panel" class:on style:--glow={currentColor && colorMode !== 'color_temp' ? currentColor : 'var(--ember)'}>
  <button class="power" class:on onclick={toggle} aria-pressed={on} aria-label={on ? 'Éteindre' : 'Allumer'}>
    <svg viewBox="0 0 24 24" width="30" height="30" aria-hidden="true"><path d="M12 3v8M6.3 6.6a8 8 0 1 0 11.4 0" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" /></svg>
  </button>
  <div class="state">
    <strong>{on ? 'Allumée' : 'Éteinte'}</strong>
    {#if on && dimmable}<span class="num">{pct} %</span>{/if}
  </div>
</section>

{#if dimmable}
  <label class="slider">
    <span>Luminosité</span>
    <input
      type="range"
      class="bright"
      min="1"
      max="254"
      value={shownBright}
      oninput={(e) => (bright = Number(e.currentTarget.value))}
      onchange={(e) => setBrightness(Number(e.currentTarget.value))}
    />
  </label>
{/if}

{#if ctRange}
  <label class="slider">
    <span>Blanc <em class="num">{kelvin(shownTemp)} K</em></span>
    <input
      type="range"
      class="temp"
      min={ctRange[0]}
      max={ctRange[1]}
      value={shownTemp}
      oninput={(e) => (temp = Number(e.currentTarget.value))}
      onchange={(e) => setTemp(Number(e.currentTarget.value))}
    />
    <span class="ends"><em>Froid</em><em>Chaud</em></span>
  </label>
{/if}

{#if hasColor}
  <div class="block">
    <span class="lbl">Couleur</span>
    <div class="swatches">
      {#each SWATCHES as c (c)}
        <button class="sw" style:background={c} aria-label="Couleur {c}" onclick={() => setColor(c)}></button>
      {/each}
      <label class="sw custom" aria-label="Autre couleur">
        <input type="color" value={currentColor ?? '#ffb35c'} onchange={(e) => setColor(e.currentTarget.value)} />
      </label>
    </div>
  </div>
{/if}

{#if effects.length}
  <div class="block">
    <span class="lbl">Effet</span>
    <div class="chips">
      {#each effects as fx (fx)}<button class="chip" onclick={() => send({ effect: fx })}>{valueLabel(fx)}</button>{/each}
    </div>
  </div>
{/if}

{#if advanced}
  <div class="block">
    <span class="lbl">Fondu des changements</span>
    <div class="seg" role="radiogroup" aria-label="Fondu">
      {#each FADES as f (f.value)}
        <button role="radio" aria-checked={fade === f.value} class:sel={fade === f.value} onclick={() => setFade(f.value)}>{f.label}</button>
      {/each}
    </div>
  </div>
{/if}

<style>
  .panel {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 16px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .panel.on {
    background: color-mix(in srgb, var(--glow) 22%, var(--surface));
  }

  .power {
    display: grid;
    place-items: center;
    width: 64px;
    height: 64px;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--muted);
    transition: background 0.2s, box-shadow 0.2s;
  }

  .power.on {
    background: var(--glow);
    color: var(--ember-ink);
    box-shadow: 0 0 0 6px color-mix(in srgb, var(--glow) 30%, transparent);
  }

  .state {
    display: grid;
  }

  .state strong {
    font-size: 20px;
  }

  .state span {
    color: var(--muted);
  }

  .slider,
  .block {
    display: grid;
    gap: 8px;
    margin-top: 18px;
  }

  .slider span,
  .lbl {
    font-weight: 600;
    display: flex;
    justify-content: space-between;
  }

  .slider em {
    font-style: normal;
    font-weight: 400;
    color: var(--muted);
  }

  .ends {
    font-size: 13px;
    margin-top: -4px;
  }

  input[type='range'] {
    -webkit-appearance: none;
    appearance: none;
    width: 100%;
    height: 34px;
    border-radius: 17px;
    background: var(--surface-2);
    outline-offset: 3px;
  }

  input.bright {
    background: linear-gradient(90deg, #2a2a2a, var(--ember-glow));
  }

  input.temp {
    background: linear-gradient(90deg, #cfe3ff, #fff6e5 45%, #ffc06a);
  }

  input[type='range']::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    background: #fff;
    border: 2px solid rgba(0, 0, 0, 0.15);
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.3);
  }

  input[type='range']::-moz-range-thumb {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    background: #fff;
    border: 2px solid rgba(0, 0, 0, 0.15);
  }

  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }

  .sw {
    width: 40px;
    height: 40px;
    border-radius: 50%;
    box-shadow: inset 0 0 0 2px rgba(255, 255, 255, 0.4), 0 1px 3px rgba(0, 0, 0, 0.2);
  }

  .custom {
    position: relative;
    background: conic-gradient(red, yellow, lime, cyan, blue, magenta, red);
    cursor: pointer;
  }

  .custom input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .chip {
    padding: 8px 14px;
    border-radius: var(--radius-chip);
    background: var(--surface-2);
    font-weight: 600;
  }

  .seg {
    display: flex;
    padding: 3px;
    border-radius: 14px;
    background: var(--surface-2);
  }

  .seg button {
    flex: 1;
    min-height: 36px;
    border-radius: 11px;
    font-size: 14px;
    font-weight: 600;
    color: var(--muted);
  }

  .seg button.sel {
    background: var(--surface);
    color: var(--ink);
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.08);
  }
</style>
