<script>
  // Un appareil qui mesure : thermomètre, détecteur d'ouverture, de présence...
  // Plus discret qu'une commande : on le lit, on n'appuie pas dessus.
  import Icon from './Icon.svelte';
  import { formatValue, iconFor, isAlerting, label } from '../lib/format.js';
  import { isAvailable } from '../lib/house.js';

  /** tint : couleur d'une règle (chaud en orange, fuite en rouge…). */
  let { device, onedit, tint = null, label: title = null } = $props();

  const main = $derived(device.readings[0]);
  const others = $derived(device.readings.slice(1, 3));
  const alert = $derived(device.readings.some(isAlerting));
  const available = $derived(isAvailable(main));
  const lowBattery = $derived(device.battery != null && device.battery <= 20);
</script>

<button class="card" class:alert class:tinted={!!tint && !alert} style:--tint={tint} class:gone={!available} onclick={onedit} aria-label="{title || device.name}, détail">
  <span class="icon"><Icon name={iconFor(main)} size={20} /></span>
  <span class="text">
    <span class="name">{title || device.name}</span>
    <span class="values num">
      <strong>{formatValue(main)}</strong>
      {#each others as r (r.entity_id)}
        <span title={label(r)}>{formatValue(r)}</span>
      {/each}
    </span>
  </span>
  {#if lowBattery}
    <span class="battery num" title="Pile faible"><Icon name="battery" size={16} /> {device.battery} %</span>
  {/if}
</button>

<style>
  .card {
    -webkit-touch-callout: none;
    user-select: none;
    -webkit-user-select: none;
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 64px;
    padding: 10px 14px 10px 10px;
    border-radius: 16px;
    background: transparent;
    border: 1px solid var(--line);
    text-align: left;
  }

  .card:hover {
    background: color-mix(in srgb, var(--surface) 60%, transparent);
  }

  .alert {
    border-color: var(--alert);
  }

  .tinted {
    border-color: color-mix(in srgb, var(--tint) 60%, transparent);
    background: color-mix(in srgb, var(--tint) 12%, transparent);
  }

  .tinted .icon {
    background: var(--tint);
    color: #fff;
  }

  .alert .icon {
    color: var(--alert);
    background: color-mix(in srgb, var(--alert) 12%, transparent);
  }

  .gone {
    opacity: 0.55;
  }

  .icon {
    flex: none;
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: 12px;
    background: var(--surface-2);
    color: var(--muted);
  }

  .text {
    display: grid;
    min-width: 0;
  }

  .name {
    font-size: 14px;
    color: var(--muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .values {
    display: flex;
    gap: 12px;
    align-items: baseline;
    font-size: 15px;
    color: var(--muted);
  }

  .values strong {
    font-size: 19px;
    color: var(--ink);
    font-weight: 600;
  }

  .alert .values strong {
    color: var(--alert);
  }

  .battery {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 13px;
    font-weight: 600;
    color: var(--alert);
    white-space: nowrap;
  }
</style>
