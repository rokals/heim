<script>
  import { tr, locale } from '../lib/i18n.js';
  // Courbe d'une valeur (température, humidité, puissance…) sur 24 h ou 7 jours.
  // Dessinée en SVG, sans bibliothèque : légère pour le Pi comme pour le téléphone.
  import { api } from '../lib/store.svelte.js';

  let { entity, unit = '' } = $props();

  let hours = $state(24);
  let points = $state([]);
  let loading = $state(true);
  let hover = $state(null);

  const W = 320;
  const H = 120;
  const PAD = { l: 34, r: 8, t: 10, b: 20 };

  $effect(() => {
    const id = entity;
    const h = hours;
    loading = true;
    api(`/history/${encodeURIComponent(id)}?hours=${h}&points=160`)
      .then((p) => (points = p))
      .catch(() => (points = []))
      .finally(() => (loading = false));
  });

  const stats = $derived.by(() => {
    if (points.length < 2) return null;
    const vs = points.map((p) => p.v);
    let min = Math.min(...vs);
    let max = Math.max(...vs);
    if (max - min < 1) {
      max += 0.5;
      min -= 0.5;
    }
    const t0 = points[0].t;
    const t1 = Math.max(points.at(-1).t, t0 + 1);
    const x = (t) => PAD.l + ((t - t0) / (t1 - t0)) * (W - PAD.l - PAD.r);
    const y = (v) => PAD.t + (1 - (v - min) / (max - min)) * (H - PAD.t - PAD.b);
    const line = points.map((p, i) => `${i ? 'L' : 'M'}${x(p.t).toFixed(1)},${y(p.v).toFixed(1)}`).join('');
    const area = `${line}L${x(points.at(-1).t).toFixed(1)},${H - PAD.b}L${x(t0).toFixed(1)},${H - PAD.b}Z`;
    return {
      min: Math.min(...vs),
      max: Math.max(...vs),
      last: vs.at(-1),
      line,
      area,
      x,
      y,
      lo: min,
      hi: max,
      t0,
      t1,
    };
  });

  const fmt = (v) => (Number.isInteger(v) ? String(v) : v.toFixed(1).replace('.', ','));
  const fmtT = (t) =>
    new Date(t * 1000).toLocaleString(locale(), hours > 24 ? { weekday: 'short', hour: '2-digit' } : { hour: '2-digit', minute: '2-digit' });

  function onMove(e) {
    if (!stats) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const px = ((e.clientX - rect.left) / rect.width) * W;
    let best = points[0];
    for (const p of points) if (Math.abs(stats.x(p.t) - px) < Math.abs(stats.x(best.t) - px)) best = p;
    hover = best;
  }
</script>

<div class="chart">
  <div class="head">
    {#if stats}
      <span class="num">{tr("min {fmt}{unit} · max {fmt2}{unit2}", { fmt: fmt(stats.min), unit, fmt2: fmt(stats.max), unit2: unit })}</span>
    {:else}
      <span></span>
    {/if}
    <div class="seg">
      <button class:on={hours === 24} onclick={() => (hours = 24)}>24 h</button>
      <button class:on={hours === 168} onclick={() => (hours = 168)}>7 j</button>
    </div>
  </div>
  {#if stats}
    <svg
      viewBox="0 0 {W} {H}"
      role="img"
      aria-label={tr("Courbe sur {v}", { v: hours === 24 ? tr("24 heures") : tr("7 jours") })}
      onpointermove={onMove}
      onpointerleave={() => (hover = null)}
    >
      <line class="grid" x1={PAD.l} x2={W - PAD.r} y1={stats.y(stats.hi)} y2={stats.y(stats.hi)} />
      <line class="grid" x1={PAD.l} x2={W - PAD.r} y1={stats.y(stats.lo)} y2={stats.y(stats.lo)} />
      <text class="lbl" x={PAD.l - 4} y={stats.y(stats.hi) + 4} text-anchor="end">{fmt(stats.hi)}</text>
      <text class="lbl" x={PAD.l - 4} y={stats.y(stats.lo) + 4} text-anchor="end">{fmt(stats.lo)}</text>
      <text class="lbl" x={PAD.l} y={H - 4}>{fmtT(stats.t0)}</text>
      <text class="lbl" x={W - PAD.r} y={H - 4} text-anchor="end">{fmtT(stats.t1)}</text>
      <path class="area" d={stats.area} />
      <path class="line" d={stats.line} />
      {#if hover}
        <line class="cursor" x1={stats.x(hover.t)} x2={stats.x(hover.t)} y1={PAD.t} y2={H - PAD.b} />
        <circle class="dot" cx={stats.x(hover.t)} cy={stats.y(hover.v)} r="3.5" />
      {/if}
    </svg>
    <p class="tip num">
      {#if hover}{fmtT(hover.t)} : <strong>{fmt(hover.v)}{unit}</strong>{:else}&nbsp;{/if}
    </p>
  {:else if loading}
    <p class="muted">{tr("Chargement…")}</p>
  {:else}
    <p class="muted">{tr("Pas encore assez de mesures : la courbe se remplit au fil des heures (un point toutes les 5 minutes au plus).")}</p>
  {/if}
</div>

<style>
  .chart {
    display: grid;
    gap: 4px;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    font-size: 13px;
    color: var(--muted);
  }

  .seg {
    display: flex;
    padding: 2px;
    border-radius: 10px;
    background: var(--surface-2);
  }

  .seg button {
    min-height: 30px;
    padding: 0 10px;
    border-radius: 8px;
    font-size: 13px;
    font-weight: 600;
    color: var(--muted);
  }

  .seg button.on {
    background: var(--surface);
    color: var(--ink);
  }

  svg {
    width: 100%;
    height: auto;
    touch-action: pan-y;
  }

  .grid {
    stroke: var(--line);
    stroke-dasharray: 3 3;
  }

  .lbl {
    font-size: 9px;
    fill: var(--muted);
  }

  .area {
    fill: color-mix(in srgb, var(--ember) 22%, transparent);
  }

  .line {
    fill: none;
    stroke: color-mix(in srgb, var(--ember) 80%, var(--ink));
    stroke-width: 2;
    stroke-linejoin: round;
  }

  .cursor {
    stroke: var(--muted);
    stroke-width: 1;
  }

  .dot {
    fill: var(--ink);
  }

  .tip {
    margin: 0;
    font-size: 13px;
    color: var(--muted);
    min-height: 18px;
  }
</style>
