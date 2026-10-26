<script>
  // Ce qu'un membre ou un invité peut faire : son tableau de bord (imposé ou libre), modifier
  // et ajouter des cartes, les onglets qu'il voit, les appareils qu'il ne peut pas commander.
  // C'est vérifié par le Raspberry Pi lui-même, pas seulement masqué dans l'interface.
  import { untrack } from 'svelte';
  import Sheet from '../Sheet.svelte';
  import Icon from '../Icon.svelte';
  import { api, home, toast } from '../../lib/store.svelte.js';
  import { iconFor } from '../../lib/format.js';

  let { user, onclose, onsaved } = $props();

  const start = untrack(() => $state.snapshot(user.access ?? {}));
  let access = $state({
    board: start.board ?? '',
    can_edit: Boolean(start.can_edit),
    can_add: Boolean(start.can_add),
    hidden_tabs: [...(start.hidden_tabs ?? [])],
    readonly: [...(start.readonly ?? [])],
  });
  let query = $state('');
  let saving = $state(false);

  const boards = $derived(home.boards?.boards ?? []);
  const controllable = $derived(
    Object.values(home.entities)
      .filter((e) => ['light', 'switch', 'siren', 'group'].includes(e.domain) && !e.hidden)
      .filter((e) => !query.trim() || `${e.name} ${e.room ?? ''}`.toLowerCase().includes(query.trim().toLowerCase()))
      .sort((a, b) => (a.room || '').localeCompare(b.room || '', 'fr') || a.name.localeCompare(b.name, 'fr')),
  );

  const TABS = [
    ['appareils', 'Appareils'],
    ['scenes', 'Scènes'],
    ['automatismes', 'Automatismes'],
  ];

  function toggle(list, id) {
    const i = access[list].indexOf(id);
    if (i >= 0) access[list].splice(i, 1);
    else access[list].push(id);
  }

  async function save() {
    saving = true;
    const body = $state.snapshot(access);
    if (!body.board) delete body.board;
    try {
      await api(`/users/${user.id}/access`, { method: 'PUT', body });
      toast(`Droits de ${user.name} enregistrés`);
      onsaved?.();
      onclose();
    } catch (e) {
      toast(e.message, 'alert');
    }
    saving = false;
  }
</script>

<Sheet title="Ce que peut faire {user.name}" {onclose}>
  <h3>Tableau de bord</h3>
  <label class="field">
    <span>Tableau</span>
    <select class="input" bind:value={access.board}>
      <option value="">Libre : tous ceux qui ne sont pas cachés</option>
      {#each boards as b (b.id)}<option value={b.id}>Imposé : {b.name}{b.hidden ? ' (caché)' : ''}</option>{/each}
    </select>
  </label>
  {#if access.board}<p class="muted small">{user.name} ne verra que ce tableau, sans pouvoir en changer.</p>{/if}
  <label class="check">
    <input type="checkbox" bind:checked={access.can_edit} />
    <span><strong>Modifier les cartes</strong><em>Déplacer, agrandir, régler, retirer</em></span>
  </label>
  <label class="check">
    <input type="checkbox" bind:checked={access.can_add} />
    <span><strong>Ajouter des cartes</strong></span>
  </label>

  <h3>Onglets visibles</h3>
  <div class="seg wrap">
    {#each TABS as [id, label] (id)}
      {@const hidden = access.hidden_tabs.includes(id)}
      <button class:off={hidden} onclick={() => toggle('hidden_tabs', id)} aria-pressed={!hidden}>
        <Icon name={hidden ? 'close' : 'check'} size={15} />
        {label}
      </button>
    {/each}
  </div>

  <h3>Appareils en lecture seule <span class="count">{access.readonly.length}</span></h3>
  <p class="muted small">{user.name} les voit mais ne peut pas les allumer ni les éteindre (chauffage, congélateur…).</p>
  <input class="input" type="search" placeholder="Chercher un appareil" bind:value={query} aria-label="Chercher un appareil" />
  <ul class="devs">
    {#each controllable as e (e.entity_id)}
      {@const ro = access.readonly.includes(e.entity_id)}
      <li>
        <label>
          <span class="ic" class:ro><Icon name={e.domain === 'group' ? 'bulb' : iconFor(e)} size={18} /></span>
          <span class="txt"><strong>{e.name}</strong><span>{e.room || 'Sans pièce'}</span></span>
          <input type="checkbox" checked={ro} onchange={() => toggle('readonly', e.entity_id)} aria-label="{e.name} en lecture seule" />
        </label>
      </li>
    {:else}
      <li class="none">Aucun appareil.</li>
    {/each}
  </ul>

  <div class="actions">
    <button class="btn" onclick={save} disabled={saving}>Enregistrer</button>
    <button class="btn quiet" onclick={onclose}>Annuler</button>
  </div>
</Sheet>

<style>
  h3 {
    margin: 18px 0 6px;
    font-size: 17px;
  }

  h3:first-child {
    margin-top: 0;
  }

  .small {
    font-size: 13px;
    margin: 0 0 8px;
  }

  .count {
    font-size: 12px;
    padding: 1px 7px;
    border-radius: 9px;
    background: var(--surface-2);
    vertical-align: middle;
  }

  .check {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 0;
  }

  .check input,
  .devs input {
    width: 22px;
    height: 22px;
    accent-color: var(--ink);
    flex: none;
  }

  .check span {
    display: grid;
  }

  .check em {
    font-style: normal;
    font-size: 13px;
    color: var(--muted);
  }

  .seg {
    display: flex;
    gap: 6px;
  }

  .seg.wrap {
    flex-wrap: wrap;
  }

  .seg button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 42px;
    padding: 0 14px;
    border-radius: var(--radius-chip);
    border: 1.5px solid var(--ink);
    background: var(--ink);
    color: var(--bg);
    font-weight: 600;
  }

  .seg button.off {
    background: none;
    border-color: var(--line);
    color: var(--muted);
    text-decoration: line-through;
  }

  .devs {
    list-style: none;
    padding: 0;
    margin: 8px 0 0;
    max-height: 340px;
    overflow: auto;
  }

  .devs li {
    border-bottom: 1px solid var(--line);
  }

  .devs label {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 2px;
  }

  .ic {
    flex: none;
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 10px;
    background: var(--surface-2);
    color: var(--muted);
  }

  .ic.ro {
    background: color-mix(in srgb, var(--alert) 16%, var(--surface-2));
    color: var(--alert);
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

  .none {
    padding: 10px 0;
    color: var(--muted);
  }

  .actions {
    display: flex;
    gap: 8px;
    margin-top: 16px;
  }
</style>
