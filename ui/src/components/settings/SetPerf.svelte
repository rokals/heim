<script>
  import { tr, locale } from '../../lib/i18n.js';
  // Performances de la box : anneaux (mémoire, processeur, carte SD, température) et ce que
  // prend chaque morceau. Relevé toutes les 5 s, seulement pendant que la page est ouverte.
  import { onDestroy, onMount } from 'svelte';
  import Gauge from '../Gauge.svelte';
  import { api } from '../../lib/store.svelte.js';
  import { COLORS } from '../../lib/tiles.js';

  let p = $state(null);
  let timer;

  async function load() {
    p = await api('/system/perf').catch(() => p);
  }

  onMount(() => {
    load();
    timer = setInterval(load, 5000);
  });
  onDestroy(() => clearInterval(timer));

  const mo = (b) => (b >= 1024 ** 3 ? tr("{v} Go", { v: (b / 1024 ** 3).toLocaleString(locale(), { maximumFractionDigits: 1 }) }) : tr("{v} Mo", { v: Math.round(b / 1024 ** 2) }));
  // Vert, puis orange à 85 %, rouge à 95 % (70 et 80 °C pour la température).
  const tone = (pct, temp = false) => (pct == null ? null : temp ? (pct >= 80 ? COLORS.red : pct >= 70 ? COLORS.orange : COLORS.green) : pct >= 95 ? COLORS.red : pct >= 85 ? COLORS.orange : COLORS.green);

  const rings = $derived(
    !p
      ? []
      : [
          p.memory && { id: 'mem', label: tr("Mémoire"), pct: p.memory.percent, text: `${Math.round(p.memory.percent)}%`, sub: tr("{a} sur {b}", { a: mo(p.memory.used), b: mo(p.memory.total) }) },
          p.cpu != null && { id: 'cpu', label: tr("Processeur"), pct: p.cpu, text: `${Math.round(p.cpu)}%`, sub: p.load ? tr("charge {v}", { v: p.load[0].toLocaleString(locale(), { maximumFractionDigits: 2 }) }) : tr("{n} cœurs", { n: p.cores }) },
          p.disk && { id: 'disk', label: tr("Carte SD"), pct: p.disk.percent, text: `${Math.round(p.disk.percent)}%`, sub: tr("{a} sur {b}", { a: mo(p.disk.used), b: mo(p.disk.total) }) },
          p.temperature != null && { id: 'temp', label: tr("Température"), pct: ((p.temperature - 20) / 70) * 100, temp: p.temperature, text: `${Math.round(p.temperature)}°`, sub: tr("processeur") },
        ].filter(Boolean),
  );
  const totalProc = $derived(p?.processes?.reduce((a, x) => a + x.memory, 0) ?? 0);
</script>

<section class="perf">
  <h2>{tr("Performances")}</h2>
  {#if !p}
    <p class="muted">{tr("Chargement…")}</p>
  {:else}
    <div class="rings">
      {#each rings as r (r.id)}
        <div class="ring" style:--accent={tone(r.temp ?? r.pct, r.temp != null)}>
          <Gauge kind="ring" fraction={r.pct / 100} size={84} label={r.text} />
          <strong>{r.label}</strong>
          <small class="num">{r.sub}</small>
        </div>
      {/each}
    </div>

    {#if p.processes?.length}
      <h3>{tr("Ce que prend chaque morceau")}</h3>
      <ul class="procs">
        {#each p.processes as x (x.name)}
          <li>
            <span class="name">{x.name}</span>
            <span class="bar"><Gauge kind="bar" fraction={p.memory ? x.memory / p.memory.total : x.memory / Math.max(totalProc, 1)} /></span>
            <span class="num v">{mo(x.memory)}{x.cpu != null ? ` · ${x.cpu.toLocaleString(locale(), { maximumFractionDigits: 1 })} %` : ''}</span>
          </li>
        {/each}
      </ul>
    {/if}
    <p class="muted small">
      {tr("Ces mesures existent aussi comme appareil « Box Heim » : ajoutez-les en tuiles (filtre puce dans Ajouter une tuile), en courbes ou dans un automatisme.")}
    </p>
  {/if}
</section>

<style>
  .perf {
    margin-bottom: 8px;
  }

  .rings {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(130px, 1fr));
    gap: 12px;
    margin: 12px 0;
  }

  .ring {
    display: grid;
    justify-items: center;
    gap: 4px;
    padding: 16px 10px 14px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
    text-align: center;
  }

  .ring small {
    font-size: 12px;
    color: var(--muted);
  }

  h3 {
    margin: 20px 0 8px;
    font-size: 16px;
  }

  .procs {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  .procs li {
    display: grid;
    grid-template-columns: minmax(90px, auto) 1fr auto;
    align-items: center;
    gap: 12px;
    padding: 9px 0;
    border-bottom: 1px solid var(--line);
    --accent: var(--ember);
  }

  .name {
    font-weight: 600;
  }

  .v {
    font-size: 13px;
    color: var(--muted);
  }

  .small {
    font-size: 13px;
  }
</style>
