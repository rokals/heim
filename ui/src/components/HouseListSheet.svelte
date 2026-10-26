<script>
  import { tr } from '../lib/i18n.js';
  // La liste derrière une carte Maison : les lumières (à allumer une par une), les prises,
  // les personnes, les piles à changer, les appareils qui ne répondent plus.
  import Sheet from './Sheet.svelte';
  import Icon from './Icon.svelte';
  import { houseEntity, kindInfo } from '../lib/housecards.js';
  import { isOn } from '../lib/house.js';
  import { formatValue, iconFor } from '../lib/format.js';
  import { sinceText } from '../lib/tiles.js';
  import { api, command, home, isMember, toast } from '../lib/store.svelte.js';

  async function rearm(e) {
    try {
      await api(`/automations/${e.entity_id.slice('automation.'.length)}/op`, { method: 'POST', body: { state: 'rearm' } });
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  let { card, onclose } = $props();

  const ent = $derived(houseEntity(card, { entities: Object.values(home.entities), security: home.security, sun: home.sun, now: home.now }));
  const toggles = $derived(['lights', 'plugs', 'room', 'leave'].includes(card.kind));
  const rows = $derived(
    [...ent.members].sort((a, b) => Number(isOn(b)) - Number(isOn(a)) || (a.room || '').localeCompare(b.room || '', 'fr') || a.name.localeCompare(b.name, 'fr')),
  );
</script>

<Sheet title={card.name || ent.name || kindInfo(card.kind).name} {onclose}>
  <p class="muted">{ent.state.attributes.text}</p>
  <ul>
    {#each rows as e (e.entity_id)}
      <li>
        <span class="ic" class:on={toggles && isOn(e)}><Icon name={e.domain === 'person' ? 'user' : e.domain === 'automation' ? 'alert' : iconFor(e)} size={18} /></span>
        <span class="txt">
          <strong>{e.name}</strong>
          <span>
            {#if card.kind === 'fired'}{tr("depuis {sinceText}", { sinceText: sinceText(e.state?.attributes?.fired_at || e.state?.last_changed, home.now) })}
            {:else if card.kind === 'people'}{e.state?.value === 'home' ? tr("À la maison") : tr("Absent")}
            {:else if card.kind === 'batteries'}{formatValue(e)}{e.room ? ` · ${e.room}` : ''}
            {:else if card.kind === 'offline'}{e.state?.last_changed ? tr("Injoignable depuis {since}", { since: sinceText(e.state.last_changed, home.now) }) : tr("Injoignable")}{e.room ? ` · ${e.room}` : ''}
            {:else}{e.room || tr("Sans pièce")}{/if}
          </span>
        </span>
        {#if card.kind === 'fired' && isMember()}
          <button class="btn quiet" onclick={() => rearm(e)}>{tr("Réarmer")}</button>
        {/if}
        {#if toggles}
          <button
            class="switch"
            class:on={isOn(e)}
            disabled={e.state?.available === false}
            onclick={() => command(e.entity_id, isOn(e) ? 'turn_off' : 'turn_on')}
            aria-label="{e.name} : {isOn(e) ? tr("éteindre") : 'allumer'}"
          ><span></span></button>
        {/if}
      </li>
    {:else}
      <li class="none">{tr("Rien à afficher.")}</li>
    {/each}
  </ul>
</Sheet>

<style>
  ul {
    list-style: none;
    margin: 8px 0 0;
    padding: 0;
  }

  li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
  }

  .none {
    color: var(--muted);
  }

  .ic {
    flex: none;
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--muted);
  }

  .ic.on {
    background: var(--ember);
    color: var(--ember-ink);
  }

  .txt {
    flex: 1;
    display: grid;
    min-width: 0;
  }

  .txt span {
    font-size: 13px;
    color: var(--muted);
  }

  .switch {
    flex: none;
    width: 52px;
    height: 32px;
    border-radius: 16px;
    background: var(--surface-2);
    position: relative;
    transition: background-color 0.2s ease;
  }

  .switch span {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    background: var(--surface);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
    transition: transform 0.2s ease;
  }

  .switch.on {
    background: var(--ember);
  }

  .switch.on span {
    transform: translateX(20px);
  }

  .switch:disabled {
    opacity: 0.4;
  }
</style>
