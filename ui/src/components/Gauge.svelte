<script>
  // Une jauge : anneau (avec la valeur au centre) ou barre de chargement.
  // fraction : 0 à 1. La couleur suit --accent (celle de la tuile ou de sa règle).
  let { kind = 'ring', fraction = 0, size = 42, label = '', thick = false } = $props();

  const R = 16;
  const C = 2 * Math.PI * R;
  // L'anneau s'ouvre en bas (300°), comme un compteur.
  const ARC = (300 / 360) * C;
  const f = $derived(Math.min(1, Math.max(0, Number(fraction) || 0)));
</script>

{#if kind === 'ring'}
  <span class="ring" style:width="{size}px" style:height="{size}px" role="img" aria-label={label}>
    <svg viewBox="0 0 40 40" aria-hidden="true">
      <circle class="track" cx="20" cy="20" r={R} stroke-dasharray="{ARC} {C}" transform="rotate(120 20 20)" />
      <circle class="fill" cx="20" cy="20" r={R} stroke-dasharray="{ARC * f} {C}" transform="rotate(120 20 20)" class:zero={f === 0} />
    </svg>
    {#if label}<span class="in num" style:font-size="{Math.max(10, Math.round(size * 0.26))}px">{label}</span>{/if}
  </span>
{:else}
  <span class="bar" class:thick role="img" aria-label={label}><i style:width="{f * 100}%"></i></span>
{/if}

<style>
  .ring {
    position: relative;
    flex: none;
    display: grid;
    place-items: center;
  }

  svg {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  circle {
    fill: none;
    stroke-width: 4;
    stroke-linecap: round;
  }

  .track {
    stroke: color-mix(in srgb, currentColor 14%, transparent);
  }

  .fill {
    stroke: var(--gauge, var(--accent, var(--ember)));
    transition: stroke-dasharray 0.6s ease;
  }

  .fill.zero {
    stroke: transparent;
  }

  .in {
    position: relative;
    font-weight: 700;
    letter-spacing: -0.02em;
    line-height: 1;
  }

  .bar {
    display: block;
    width: 100%;
    height: 8px;
    border-radius: 99px;
    background: color-mix(in srgb, currentColor 12%, transparent);
    overflow: hidden;
  }

  .bar.thick {
    height: 12px;
  }

  .bar i {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--gauge, var(--accent, var(--ember)));
    transition: width 0.6s ease;
  }
</style>
