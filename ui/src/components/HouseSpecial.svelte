<script>
  // Les cartes Maison qui ne ressemblent pas à un bouton : la barre des scènes, les
  // minuteurs, l'horloge. Même cadre que les autres cartes, contenu à elles.
  import Icon from './Icon.svelte';
  import TimerTile from './TimerTile.svelte';
  import { isOn } from '../lib/house.js';
  import { command, home } from '../lib/store.svelte.js';
  import { cardSize } from '../lib/layout.js';

  /** preview : dans l'éditeur, rien ne part vers les appareils. */
  let { card, preview = false } = $props();

  const size = $derived(cardSize(card));
  const all = $derived(Object.values(home.entities));
  const scenes = $derived(
    all.filter((e) => e.domain === 'scene' && !e.hidden).sort((a, b) => a.name.localeCompare(b.name, 'fr')),
  );
  const timers = $derived.by(() => {
    const rank = (t) => (['running', 'paused', 'finished'].includes(t.state?.value) ? 0 : 1);
    return all
      .filter((e) => e.domain === 'timer' && !e.hidden && !e.entity_id.startsWith('timer.auto_'))
      .concat(all.filter((e) => e.entity_id.startsWith('timer.auto_') && e.state?.value === 'running'))
      .sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name, 'fr'));
  });
  const now = $derived(new Date(home.now));
  const time = $derived(now.toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' }));
  const date = $derived(now.toLocaleDateString('fr-FR', { weekday: 'long', day: 'numeric', month: 'long' }));

  function scene(s) {
    if (preview) return;
    command(s.entity_id, isOn(s) ? 'turn_off' : 'turn_on');
  }
</script>

{#if card.kind === 'scenes'}
  <div class="frame scenes" class:wrap={size.h > 1} data-noswipe role="group" aria-label={card.name || 'Scènes'}>
    {#each scenes as s (s.entity_id)}
      {@const active = isOn(s)}
      <button class="scene" class:active style:--tint={s.capabilities?.color || null} aria-pressed={active} onclick={() => scene(s)}>
        <Icon name={s.capabilities?.icon || 'scene'} size={17} />
        {s.name}
        {#if s.state?.attributes?.partial}<span class="partial">en partie</span>{/if}
      </button>
    {:else}
      <p class="empty">Pas encore de scène : créez-en une dans l'onglet Scènes.</p>
    {/each}
  </div>
{:else if card.kind === 'timers'}
  <div class="frame timers" data-noswipe>
    {#each timers as t (t.entity_id)}<TimerTile entity={t} compact />{:else}<p class="empty">Aucun minuteur.</p>{/each}
  </div>
{:else if card.kind === 'clock'}
  <div class="frame clock" class:big={size.h >= 2} class:row={size.h === 1}>
    <strong class="num">{time}</strong>
    {#if size.w >= 2}<span>{date}</span>{/if}
  </div>
{/if}

<style>
  .frame {
    height: 100%;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
    overflow: hidden;
  }

  .scenes {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .scenes.wrap {
    flex-wrap: wrap;
    align-content: center;
    overflow-y: auto;
    padding: 12px;
  }

  .scene {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 16px;
    border-radius: var(--radius-chip);
    border: 1.5px solid var(--line);
    font-weight: 600;
    white-space: nowrap;
  }

  .scene.active {
    background: var(--tint, var(--ember));
    border-color: var(--tint, var(--ember));
    color: var(--ember-ink);
  }

  .scene.active[style*='--tint'] {
    color: #fff;
  }

  .partial {
    font-size: 12px;
    font-weight: 400;
  }

  .timers {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(240px, 100%), 1fr));
    align-content: start;
    gap: 8px;
    padding: 8px;
    overflow-y: auto;
    background: transparent;
    box-shadow: none;
  }

  .empty {
    margin: 0;
    padding: 0 6px;
    font-size: 14px;
    color: var(--muted);
  }

  .timers .empty {
    padding: 14px;
  }

  .clock {
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 2px;
    padding: 8px;
    text-align: center;
  }

  .clock.row {
    display: flex;
    align-items: baseline;
    justify-content: center;
    gap: 12px;
  }

  .clock strong {
    font-size: 26px;
    font-weight: 650;
    letter-spacing: -0.02em;
    line-height: 1;
  }

  .clock.big strong {
    font-size: clamp(36px, 12cqw, 72px);
  }

  .clock span {
    color: var(--muted);
    font-size: 14px;
    text-transform: capitalize;
  }
</style>
