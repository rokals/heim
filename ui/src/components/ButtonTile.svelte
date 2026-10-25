<script>
  // Le bouton à tout faire de l'accueil : une scène, un automatisme (son état ou son
  // interrupteur), un groupe, un capteur, une personne… Il montre une valeur, prend la couleur,
  // l'icône et le texte de la première règle vraie, et agit à l'appui (l'appui long est géré
  // par la page, pour qu'il marche pareil sur toutes les cartes).
  import Icon from './Icon.svelte';
  import { appearance, domainOf } from '../lib/tiles.js';
  import { home } from '../lib/store.svelte.js';

  let { card, entity, wide = false, ontap } = $props();

  const look = $derived(appearance(card, entity, home.now));
  const available = $derived(entity?.state?.available !== false);
  const name = $derived(card.name || entity?.name || card.entity_id);
  const pending = $derived(entity && entity.entity_id in home.pending);
  const d = $derived(domainOf(card.entity_id));
  const disabledAuto = $derived(d === 'automation' && entity?.state?.attributes?.enabled === false);
  const sub = $derived.by(() => {
    if (d === 'group') {
      const n = entity?.state?.attributes?.on_count ?? 0;
      const total = entity?.state?.attributes?.members?.length ?? 0;
      return look.active && total ? `${n} sur ${total}` : null;
    }
    if (d === 'automation' && card.facet !== 'enabled' && disabledAuto) return 'désactivé';
    return null;
  });
</script>

<button
  class="tile"
  class:wide
  class:active={look.active && !look.color}
  class:colored={!!look.color}
  class:gone={!available}
  style:--tint={look.color}
  onclick={ontap}
  aria-label="{name} : {look.text}"
>
  <span class="icon" class:spin={pending}><Icon name={look.icon} size={22} /></span>
  <span class="txt">
    <span class="name">{name}</span>
    <span class="value num">{look.text}{#if sub}<span class="sub">&nbsp;· {sub}</span>{/if}</span>
  </span>
</button>

<style>
  .tile {
    position: relative;
    display: grid;
    align-content: space-between;
    gap: 12px;
    width: 100%;
    min-height: 150px;
    padding: 16px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
    text-align: left;
    transition: background-color 0.35s ease, color 0.35s ease;
    -webkit-touch-callout: none;
    user-select: none;
    -webkit-user-select: none;
  }

  .wide {
    grid-template-columns: auto 1fr;
    align-items: center;
    align-content: center;
    min-height: 76px;
    padding: 12px 16px;
  }

  .icon {
    display: inline-grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--muted);
    transition: background-color 0.35s ease, color 0.35s ease;
  }

  .txt {
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  .name {
    font-size: 17px;
    font-weight: 600;
    line-height: 1.2;
    overflow-wrap: anywhere;
  }

  .value {
    font-size: 14px;
    color: var(--muted);
  }

  .sub {
    font-weight: 400;
  }

  /* Actif sans couleur choisie : la braise, comme une lampe allumée. */
  .active {
    background: color-mix(in srgb, var(--ember) 62%, var(--ember-glow));
    color: var(--ember-ink);
  }

  .active .icon {
    background: color-mix(in srgb, var(--ember-glow) 60%, white);
    color: var(--ember-ink);
  }

  .active .value {
    color: color-mix(in srgb, var(--ember-ink) 75%, transparent);
  }

  /* Couleur d'une règle : fond teinté, pastille pleine. */
  .colored {
    background: color-mix(in srgb, var(--tint) 22%, var(--surface));
    box-shadow: var(--shadow), inset 0 0 0 1.5px color-mix(in srgb, var(--tint) 55%, transparent);
  }

  .colored .icon {
    background: var(--tint);
    color: #fff;
  }

  .colored .value {
    color: color-mix(in srgb, var(--tint) 70%, var(--ink));
    font-weight: 600;
  }

  .gone {
    opacity: 0.55;
  }

  .spin {
    animation: breathe 1.2s ease-in-out infinite;
  }

  @keyframes breathe {
    50% {
      opacity: 0.45;
    }
  }
</style>
