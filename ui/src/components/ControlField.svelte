<script>
  // Un réglage d'appareil, quel qu'il soit : interrupteur, choix, curseur, texte.
  import { controlLabel, showDescription, valueLabel } from '../lib/controls.js';

  let { control: c, value, onchange, disabled = false } = $props();

  const label = $derived(controlLabel(c));
  const values = $derived(c.values ?? []);
  const segmented = $derived(c.kind === 'enum' && values.length <= 4 && values.every((v) => String(valueLabel(v)).length <= 14));
  const on = $derived(c.kind === 'binary' && (value === (c.value_on ?? true)));
  const sliderOk = $derived(c.kind === 'numeric' && c.min != null && c.max != null && c.max - c.min <= 100000);
  let draft = $state(null);
  const shown = $derived(draft ?? value);

  function commit(v) {
    draft = null;
    onchange(v);
  }

  function fmt(v) {
    if (v == null || v === '') return '–';
    if (typeof v === 'string' && Number.isNaN(Number(v))) return valueLabel(v);
    const n = Number(v);
    const s = Number.isInteger(n) ? String(n) : n.toFixed(c.step && c.step < 1 ? String(c.step).split('.')[1]?.length ?? 1 : 1);
    return c.unit ? `${s} ${c.unit}` : s;
  }
</script>

<div class="field-row" class:col={c.kind !== 'binary'}>
  <div class="head">
    <span class="name">{label}</span>
    {#if c.kind === 'numeric'}<span class="val num">{fmt(shown)}</span>{/if}
  </div>

  {#if c.kind === 'binary'}
    <button
      type="button"
      class="switch"
      class:on
      role="switch"
      aria-checked={on}
      aria-label={label}
      {disabled}
      onclick={() => commit(on ? (c.value_off ?? false) : (c.value_on ?? true))}
    ><span></span></button>
  {:else if c.kind === 'enum' && segmented}
    <div class="seg" role="radiogroup" aria-label={label}>
      {#each values as v (v)}
        <button type="button" role="radio" aria-checked={value === v} class:sel={value === v} {disabled} onclick={() => commit(v)}>{valueLabel(v)}</button>
      {/each}
    </div>
  {:else if c.kind === 'enum'}
    <select class="input" value={value ?? ''} {disabled} onchange={(e) => commit(e.currentTarget.value)} aria-label={label}>
      {#if value == null}<option value="" disabled>Choisir…</option>{/if}
      {#each values as v (v)}<option value={v}>{valueLabel(v)}</option>{/each}
    </select>
  {:else if sliderOk}
    <input
      type="range"
      min={c.min}
      max={c.max}
      step={c.step ?? 1}
      value={typeof shown === 'number' ? shown : c.min}
      {disabled}
      aria-label={label}
      oninput={(e) => (draft = Number(e.currentTarget.value))}
      onchange={(e) => commit(Number(e.currentTarget.value))}
    />
    {#if c.presets?.length}
      <div class="presets">
        {#each c.presets as p (p.name)}<button type="button" class="chip" {disabled} onclick={() => commit(p.value)}>{valueLabel(p.name)}</button>{/each}
      </div>
    {/if}
  {:else if c.kind === 'numeric'}
    <input class="input num" type="number" min={c.min} max={c.max} step={c.step ?? 'any'} value={value ?? ''} {disabled} aria-label={label} onchange={(e) => commit(Number(e.currentTarget.value))} />
  {:else}
    <input class="input" value={value ?? ''} {disabled} aria-label={label} onchange={(e) => commit(e.currentTarget.value)} />
  {/if}
  {#if showDescription(c) && c.kind !== 'binary'}<p class="desc">{c.description}</p>{/if}
</div>

<style>
  .field-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 14px 0;
    border-bottom: 1px solid var(--line);
  }

  .field-row.col {
    flex-direction: column;
    align-items: stretch;
  }

  .head {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }

  .name {
    font-weight: 600;
  }

  .val {
    color: var(--muted);
  }

  .desc {
    margin: 0;
    font-size: 13px;
    color: var(--muted);
  }

  .switch {
    flex: none;
    position: relative;
    width: 52px;
    height: 32px;
    border-radius: 16px;
    background: var(--line);
    transition: background 0.15s;
  }

  .switch span {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.25);
    transition: transform 0.15s;
  }

  .switch.on {
    background: var(--moss);
  }

  .switch.on span {
    transform: translateX(20px);
  }

  .seg {
    display: flex;
    padding: 3px;
    border-radius: 14px;
    background: var(--surface-2);
  }

  .seg button {
    flex: 1;
    min-height: 38px;
    padding: 0 6px;
    border-radius: 11px;
    font-size: 14px;
    font-weight: 600;
    color: var(--muted);
  }

  .seg button.sel {
    background: var(--surface);
    color: var(--ink);
    box-shadow: var(--shadow), 0 1px 4px rgba(0, 0, 0, 0.08);
  }

  input[type='range'] {
    width: 100%;
    accent-color: var(--ink);
  }

  .presets {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chip {
    padding: 5px 12px;
    border-radius: var(--radius-chip);
    background: var(--surface-2);
    font-size: 13px;
    font-weight: 600;
  }
</style>
