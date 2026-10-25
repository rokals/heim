<script>
  // Le détail de ce qui n'est pas un appareil : un automatisme (état, réarmer, activer,
  // pourquoi il ne s'est pas lancé), une scène, un groupe, une personne. Avec son journal.
  import Sheet from './Sheet.svelte';
  import Icon from './Icon.svelte';
  import JournalList from './JournalList.svelte';
  import { api, command, home, isAdmin, isMember, toast } from '../lib/store.svelte.js';
  import { domainOf, sinceText } from '../lib/tiles.js';
  import { when } from '../lib/journal.js';

  let { entityId, onclose, onopen = null } = $props();

  const entity = $derived(home.entities[entityId]);
  const d = $derived(domainOf(entityId));
  const attrs = $derived(entity?.state?.attributes ?? {});
  const autoId = $derived(d === 'automation' ? entityId.slice('automation.'.length) : null);
  const on = $derived(entity?.state?.value === 'on');
  let busy = $state(false);

  async function op(body, done) {
    busy = true;
    try {
      await api(`/automations/${autoId}/op`, { method: 'POST', body });
      if (done) toast(done);
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  const members = $derived(
    (attrs.members ?? entity?.capabilities?.members ?? []).map((id) => home.entities[id]).filter(Boolean),
  );
</script>

<Sheet title={entity?.name ?? entityId} {onclose}>
  {#if !entity}
    <p class="muted">Introuvable : supprimé entre-temps ?</p>
  {:else if d === 'automation'}
    <div class="state" class:fired={attrs.fired}>
      <span class="big">{on ? 'Vrai' : 'Faux'}</span>
      <span class="muted">
        {#if attrs.fired && attrs.fired_at}déclenché il y a {sinceText(attrs.fired_at, home.now)}{:else}au repos{/if}
        · {attrs.enabled === false ? 'désactivé' : 'activé'}
      </span>
    </div>
    {#if isMember()}
      <div class="actions">
        {#if attrs.fired}
          <button class="btn" disabled={busy} onclick={() => op({ state: 'rearm' }, 'Réarmé')}><Icon name="restart" size={17} /> Réarmer</button>
        {/if}
        <button class="btn quiet" disabled={busy} onclick={() => op({ enabled: attrs.enabled === false }, attrs.enabled === false ? 'Activé' : 'Désactivé')}>
          {attrs.enabled === false ? 'Activer' : 'Désactiver'}
        </button>
        <button class="btn quiet" disabled={busy} onclick={() => op({ run: true }, 'Lancé')}><Icon name="play" size={16} /> Lancer maintenant</button>
        {#if isAdmin() && onopen}
          <button class="btn quiet" onclick={() => onopen(autoId)}><Icon name="pencil" size={16} /> Modifier</button>
        {/if}
      </div>
    {/if}
    <dl>
      {#if attrs.last_run}<dt>Dernière exécution</dt><dd>{when(attrs.last_run, new Date(home.now))}</dd>{/if}
      {#if attrs.last_check}
        <dt>Dernier déclenchement</dt>
        <dd>
          {when(attrs.last_check.at, new Date(home.now))} :
          {#if attrs.last_check.ran}lancé{:else}<span class="warn">pas lancé, {attrs.last_check.reason}</span>{/if}
        </dd>
      {/if}
      {#if attrs.last_error}<dt>Dernière erreur</dt><dd class="warn">{attrs.last_error}</dd>{/if}
    </dl>
  {:else if d === 'scene' || d === 'group'}
    <div class="state" class:lit={on}>
      <span class="big">{on ? (d === 'scene' ? 'Active' : 'Allumé') : d === 'scene' ? 'Inactive' : 'Éteint'}</span>
      {#if d === 'group'}<span class="muted">{attrs.on_count ?? 0} sur {members.length} allumé(s)</span>{/if}
    </div>
    <div class="actions">
      <button class="btn" onclick={() => command(entityId, on ? 'turn_off' : 'turn_on')}>
        {on ? (d === 'scene' ? 'Désactiver' : 'Éteindre') : d === 'scene' ? 'Activer' : 'Allumer'}
      </button>
    </div>
    {#if d === 'group' && members.length}
      <ul class="members">
        {#each members as m (m.entity_id)}
          <li>
            <span>{m.name}</span>
            <button class="pill" class:on={m.state?.value === 'on'} onclick={() => command(m.entity_id, 'toggle')}>
              {m.state?.value === 'on' ? 'Allumé' : 'Éteint'}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  {:else}
    <div class="state">
      <span class="big">{entity.state?.value === 'home' ? 'À la maison' : entity.state?.value === 'away' ? 'Absent' : entity.state?.value ?? '–'}</span>
      {#if entity.state?.last_changed}<span class="muted">depuis {sinceText(entity.state.last_changed, home.now)}</span>{/if}
    </div>
  {/if}

  {#if entity && isMember()}
    <h3>Historique</h3>
    <JournalList entity={entityId} limit={20} compact />
  {/if}
</Sheet>

<style>
  .state {
    display: grid;
    gap: 2px;
    padding: 16px;
    border-radius: 18px;
    background: var(--surface-2);
    margin-bottom: 12px;
  }

  .state.fired {
    background: color-mix(in srgb, #e5484d 16%, var(--surface));
  }

  .state.lit {
    background: color-mix(in srgb, var(--ember) 40%, var(--surface));
  }

  .big {
    font-size: 28px;
    font-weight: 700;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 12px;
  }

  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 6px 12px;
    margin: 0 0 8px;
    font-size: 15px;
  }

  dt {
    color: var(--muted);
  }

  dd {
    margin: 0;
  }

  .warn {
    color: var(--alert);
  }

  h3 {
    margin: 18px 0 4px;
    font-size: 17px;
  }

  .members {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  .members li {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 0;
    border-bottom: 1px solid var(--line);
  }

  .pill {
    min-height: 34px;
    padding: 0 12px;
    border-radius: var(--radius-chip);
    background: var(--surface-2);
    font-weight: 600;
    font-size: 14px;
  }

  .pill.on {
    background: var(--ember);
    color: var(--ember-ink);
  }
</style>
