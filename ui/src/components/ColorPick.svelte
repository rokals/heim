<script>
  import { tr } from '../lib/i18n.js';
  // Une rangée de pastilles de couleur : automatique (ou aucune), la palette, la couleur
  // d'une scène si on en connaît, et une couleur libre.
  import Icon from './Icon.svelte';
  import { COLORS, COLOR_NAMES } from '../lib/tiles.js';

  /**
   * value : un nom de la palette, un #rrggbb ou '' ; empty : 'auto' (A) ou 'none' (hachuré).
   * extra : [{ hex, label }] proposés en plus (ex. la couleur de la scène).
   */
  let { value = '', onchange, empty = 'auto', extra = [], label = tr("Couleur") } = $props();

  const isHex = (v) => /^#[0-9a-f]{6}$/i.test(v ?? '');
  const custom = $derived(isHex(value) && !extra.some((x) => x.hex.toLowerCase() === value.toLowerCase()));
</script>

<div class="colors" role="radiogroup" aria-label={label}>
  {#if empty === 'auto'}
    <button class="sw auto" class:sel={!value} onclick={() => onchange('')} title={tr("Automatique")} aria-label={tr("Automatique")}>A</button>
  {:else}
    <button class="sw none" class:sel={!value} onclick={() => onchange('')} title={tr("Sans couleur")} aria-label={tr("Sans couleur")}></button>
  {/if}
  {#each extra as x (x.hex)}
    <button class="sw ring" class:sel={value?.toLowerCase() === x.hex.toLowerCase()} style:background={x.hex} onclick={() => onchange(x.hex)} title={x.label} aria-label={x.label}></button>
  {/each}
  {#each Object.entries(COLORS) as [id, hex] (id)}
    <button class="sw" class:sel={value === id} style:background={hex} onclick={() => onchange(id)} title={COLOR_NAMES[id]} aria-label={COLOR_NAMES[id]}></button>
  {/each}
  <label class="sw pick" class:sel={custom} title={tr("Autre couleur")} style:background={custom ? value : null}>
    <input type="color" value={custom ? value : '#f0a93a'} onchange={(e) => onchange(e.currentTarget.value)} aria-label={tr("Autre couleur")} />
    {#if !custom}<Icon name="plus" size={14} />{/if}
  </label>
</div>

<style>
  .colors {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .sw {
    position: relative;
    display: grid;
    place-items: center;
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

  .sw.ring {
    outline: 2px dashed var(--muted);
    outline-offset: 2px;
  }

  .sw.pick {
    background: var(--surface-2);
    color: var(--muted);
    cursor: pointer;
    overflow: hidden;
  }

  .sw.pick input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
  }

  .sw.sel {
    border-color: var(--ink);
    transform: scale(1.08);
  }
</style>
