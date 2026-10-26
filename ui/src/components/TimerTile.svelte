<script>
  import { tr } from '../lib/i18n.js';
  // Un minuteur : l'anneau se vide au fil du temps. Le décompte est calculé ici
  // à partir de l'heure de fin ; le noyau n'envoie rien chaque seconde.
  import Icon from './Icon.svelte';
  import { api, home, toast } from '../lib/store.svelte.js';
  import { clock, duration, timerRemaining } from '../lib/format.js';

  let { entity, compact = false } = $props();

  const id = $derived(entity.entity_id.slice('timer.'.length));
  const value = $derived(entity.state?.value ?? 'idle');
  const total = $derived(entity.state?.attributes?.duration_s ?? 0);
  const remaining = $derived(timerRemaining(entity, home.now));
  const fraction = $derived(
    value === 'running' || value === 'paused' ? Math.min(1, remaining / Math.max(total, remaining, 1)) : value === 'finished' ? 0 : 1,
  );
  const action = $derived(entity.state?.attributes?.on_finish);
  const target = $derived(action ? home.entities[action.entity_id] : null);

  const R = 26;
  const C = 2 * Math.PI * R;

  async function op(name) {
    try {
      await api(`/timers/${id}/control`, { method: 'POST', body: { op: name } });
    } catch (e) {
      toast(e.message, 'alert');
    }
  }
</script>

<article class="timer" class:running={value === 'running'} class:finished={value === 'finished'} class:compact>
  <svg class="ring" viewBox="0 0 64 64" aria-hidden="true">
    <circle cx="32" cy="32" r={R} class="track" />
    <circle cx="32" cy="32" r={R} class="fill" stroke-dasharray={C} stroke-dashoffset={C * (1 - fraction)} />
  </svg>
  <div class="body">
    <span class="name">{entity.name}</span>
    <span class="time num">
      {#if value === 'finished'}{tr("Terminé")}{:else if value === 'idle'}{duration(total)}{:else}{clock(remaining)}{/if}
    </span>
    {#if target && !compact}
      <span class="then">{tr("puis {v} {name}", { v: action.action === 'turn_off' ? tr("éteint") : action.action === 'turn_on' ? 'allume' : 'bascule', name: target.name })}</span>
    {/if}
  </div>
  <div class="actions">
    {#if value === 'running'}
      <button class="round" onclick={() => op('pause')} aria-label={tr("Mettre en pause")}><Icon name="pause" size={18} /></button>
      <button class="round quiet" onclick={() => op('cancel')} aria-label={tr("Annuler")}><Icon name="close" size={18} /></button>
    {:else if value === 'paused'}
      <button class="round" onclick={() => op('resume')} aria-label={tr("Reprendre")}><Icon name="play" size={18} /></button>
      <button class="round quiet" onclick={() => op('cancel')} aria-label={tr("Annuler")}><Icon name="close" size={18} /></button>
    {:else}
      <button class="round" onclick={() => op('start')} aria-label={tr("Démarrer {name}", { name: entity.name })}><Icon name="play" size={18} /></button>
    {/if}
  </div>
</article>

<style>
  .timer {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 12px 14px 12px 12px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .ring {
    flex: none;
    width: 56px;
    height: 56px;
    transform: rotate(-90deg);
  }

  .track {
    fill: none;
    stroke: var(--surface-2);
    stroke-width: 6;
  }

  .fill {
    fill: none;
    stroke: var(--muted);
    stroke-width: 6;
    stroke-linecap: round;
    transition: stroke-dashoffset 0.5s linear;
  }

  .running .fill {
    stroke: var(--ember);
  }

  .finished .fill {
    stroke: var(--moss);
  }

  .body {
    display: grid;
    min-width: 0;
    flex: 1;
  }

  .name {
    font-size: 14px;
    color: var(--muted);
  }

  .time {
    font-size: 26px;
    font-weight: 600;
    line-height: 1.15;
  }

  .compact .time {
    font-size: 22px;
  }

  .then {
    font-size: 13px;
    color: var(--muted);
  }

  .actions {
    display: flex;
    gap: 6px;
  }

  .round {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: 50%;
    background: var(--ink);
    color: var(--bg);
  }

  .round.quiet {
    background: var(--surface-2);
    color: var(--ink);
  }
</style>
