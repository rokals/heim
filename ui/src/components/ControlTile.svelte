<script>
  // Une lumière ou une prise. Allumée, elle diffuse une lueur chaude proportionnelle
  // à sa luminosité : on voit d'un coup d'œil quelles pièces sont éclairées.
  import Icon from './Icon.svelte';
  import { command, home, turnOnFor } from '../lib/store.svelte.js';
  import { autoRemaining, isAvailable, isOn } from '../lib/house.js';
  import { clock, formatValue } from '../lib/format.js';

  /** forMin : durée réglée sur la carte (« un clic = allumer 30 min »). onfor : choisir une durée. */
  /** ontap : remplace la bascule (bouton réglé sur « Allumer », « Ouvrir le détail »…). */
  let { entity, device, onedit, onfor, forMin = null, wide = false, ontap = null, label = null, iconName = null } = $props();

  const on = $derived(isOn(entity));
  const available = $derived(isAvailable(entity));
  const pending = $derived(entity.entity_id in home.pending);
  const dimmable = $derived(entity.domain === 'light' && entity.capabilities?.brightness);
  const brightness = $derived(entity.state?.attributes?.brightness ?? 255);
  const glow = $derived(on ? (dimmable ? 0.35 + (brightness / 255) * 0.65 : 1) : 0);
  const power = $derived(device?.readings?.find((r) => r.capabilities?.kind === 'power'));
  const lowBattery = $derived(device?.battery != null && device.battery <= 20);

  let dragValue = $state(null);

  const remaining = $derived(autoRemaining(entity.entity_id, home.entities, home.now));

  function toggle() {
    if (ontap) return ontap();
    if (!available) return;
    if (!on && forMin) turnOnFor(entity.entity_id, forMin);
    else command(entity.entity_id, 'toggle');
  }

  function setBrightness(e) {
    const value = Number(e.currentTarget.value);
    dragValue = null;
    command(entity.entity_id, 'turn_on', { brightness: value });
  }

  const status = $derived.by(() => {
    if (!available) return 'Injoignable';
    if (pending) return on ? 'Extinction…' : 'Allumage…';
    if (!on) return forMin ? `${forMin} min d'un clic` : 'Éteint';
    if (remaining != null) return clock(remaining);
    if (dimmable) return `${Math.round(((dragValue ?? brightness) / 255) * 100)} %`;
    return 'Allumé';
  });
</script>

<article class="tile" class:on class:off={!on} class:gone={!available} class:wide style:--glow={glow}>
  <button class="hit" onclick={toggle} disabled={!available && !ontap} aria-pressed={on} aria-label="{label || entity.name} : {on ? 'éteindre' : 'allumer'}">
    <span class="icon" class:spin={pending}><Icon name={iconName || (entity.domain === 'light' ? 'bulb' : 'plug')} size={24} /></span>
    <span class="name">{label || entity.name}</span>
    <span class="status num" title={remaining != null ? "S'éteindra tout seul à la fin du décompte" : null}>
      {#if remaining != null && on}<Icon name="timer" size={15} />{/if}
      {status}
      {#if power && on && remaining == null}<span class="power">{formatValue(power)}</span>{/if}
    </span>
  </button>

  <div class="tools">
    {#if onfor && available}
      <button class="tool" onclick={onfor} aria-label="Allumer {entity.name} pour un temps" title="Allumer pour…"><Icon name="timer" size={17} /></button>
    {/if}
    {#if onedit}
      <button class="tool" onclick={onedit} aria-label="Modifier {entity.name}"><Icon name="pencil" size={16} /></button>
    {/if}
  </div>

  {#if lowBattery}
    <span class="battery" title="Pile faible">{device.battery} %</span>
  {/if}

  {#if dimmable && on && available}
    <input
      class="dim"
      type="range"
      min="3"
      max="255"
      value={dragValue ?? brightness}
      oninput={(e) => (dragValue = Number(e.currentTarget.value))}
      onchange={setBrightness}
      aria-label="Luminosité de {entity.name}"
    />
  {/if}
</article>

<style>
  .tile {
    -webkit-touch-callout: none;
    user-select: none;
    -webkit-user-select: none;
    position: relative;
    min-height: 150px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
    overflow: hidden;
    isolation: isolate;
    transition: background-color 0.4s ease;
  }

  /* Allumée : la tuile prend la couleur de la braise, et une flaque de lumière
     part du coin de l'ampoule, d'autant plus grande que la luminosité est forte. */
  .on {
    color: var(--ember-ink);
    background: color-mix(in srgb, var(--ember) 62%, var(--ember-glow));
  }

  .tile::before {
    content: '';
    position: absolute;
    inset: 0;
    z-index: -1;
    background: radial-gradient(140% 110% at 16% 10%, #fff3d6 0%, var(--ember-glow) 30%, transparent 75%);
    opacity: var(--glow);
    transition: opacity 0.5s ease;
  }

  .gone {
    opacity: 0.55;
  }

  .hit {
    display: grid;
    align-content: space-between;
    width: 100%;
    min-height: 150px;
    padding: 16px 16px 44px;
    text-align: left;
    gap: 14px;
  }

  .hit:disabled {
    cursor: not-allowed;
  }

  .icon {
    display: inline-grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--muted);
    transition: background-color 0.4s ease, color 0.4s ease;
  }

  .on .icon {
    background: color-mix(in srgb, var(--ember-glow) 60%, white);
    color: var(--ember-ink);
  }

  .spin {
    animation: breathe 1.2s ease-in-out infinite;
  }

  @keyframes breathe {
    50% {
      opacity: 0.45;
    }
  }

  .name {
    font-size: 17px;
    font-weight: 600;
    line-height: 1.2;
    overflow-wrap: anywhere;
  }

  .status {
    position: absolute;
    left: 16px;
    bottom: 14px;
    font-size: 14px;
    color: var(--muted);
    display: flex;
    align-items: center;
    gap: 6px 10px;
    white-space: nowrap;
  }

  .on .status {
    color: color-mix(in srgb, var(--ember-ink) 75%, transparent);
  }

  .power {
    font-weight: 600;
  }

  .tools {
    position: absolute;
    top: 8px;
    right: 8px;
    display: flex;
  }

  .tool {
    width: 38px;
    height: 38px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: var(--muted);
    opacity: 0.75;
  }

  .on .tool {
    color: var(--ember-ink);
  }

  .tool:hover,
  .tool:focus-visible {
    opacity: 1;
    background: color-mix(in srgb, currentColor 10%, transparent);
  }

  .wide .hit {
    grid-template-columns: auto 1fr;
    align-items: center;
    align-content: start;
  }

  .battery {
    position: absolute;
    top: 58px;
    right: 18px;
    font-size: 12px;
    font-weight: 600;
    color: var(--alert);
  }

  .dim {
    position: absolute;
    left: 50%;
    right: 14px;
    bottom: 12px;
    width: calc(50% - 14px);
    accent-color: var(--ember-ink);
    height: 22px;
  }
</style>
