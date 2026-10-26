<script>
  // Les tableaux de bord (admin) : créer, renommer, choisir l'icône, cacher de la navigation
  // (un tableau pour une tablette), ordonner, dupliquer, supprimer.
  import { untrack } from 'svelte';
  import Sheet from './Sheet.svelte';
  import Icon from './Icon.svelte';
  import { home, saveBoards, showBoard } from '../lib/store.svelte.js';

  let { onclose = null, inline = false } = $props();

  const ICONS = ['house', 'sofa', 'kitchen', 'bed', 'bath', 'desk', 'child', 'tree', 'bulb', 'shield', 'thermo', 'star', 'heart', 'car', 'laundry', 'stairs'];

  let list = $state(untrack(() => $state.snapshot(home.boards?.boards ?? [])));
  let picking = $state(null);
  let saving = $state(false);
  const dirty = $derived(JSON.stringify(list) !== JSON.stringify(home.boards?.boards ?? []));

  // Une modification venue d'un autre téléphone : on repart de la liste à jour si rien n'est en cours.
  $effect(() => {
    const fresh = home.boards?.boards;
    untrack(() => {
      if (!saving && fresh && !dirty) list = $state.snapshot(fresh);
    });
  });

  function add(kind) {
    const n = list.filter((b) => b.name.startsWith('Tableau')).length + 1;
    const base = { id: '', name: `Tableau ${n}`, icon: 'star', hidden: false, auto: false, sections: [] };
    if (kind === 'auto') Object.assign(base, { name: 'Par pièce', icon: 'house', auto: true });
    if (kind === 'empty') base.sections = [{ title: 'Section', cards: [] }];
    list.push(base);
  }

  function duplicate(k) {
    const b = $state.snapshot(list[k]);
    list.splice(k + 1, 0, { ...b, id: '', name: `${b.name} (copie)`, hidden: false });
  }

  function move(k, d) {
    const j = k + d;
    if (j < 0 || j >= list.length) return;
    [list[k], list[j]] = [list[j], list[k]];
  }

  function remove(k) {
    const b = list[k];
    if (list.length === 1) return;
    if (!confirm(`Supprimer le tableau « ${b.name} » ? Ses cartes seront perdues. Les écrans qui l'affichaient reviendront au premier tableau.`)) return;
    list.splice(k, 1);
  }

  async function save() {
    saving = true;
    const saved = await saveBoards($state.snapshot(list));
    saving = false;
    if (saved) {
      list = $state.snapshot(home.boards?.boards ?? []);
      if (!inline) onclose?.();
    }
  }
</script>

{#snippet body()}
  <p class="muted small">
    Glissez à gauche ou à droite sur l'accueil pour passer d'un tableau à l'autre (ou des onglets, au choix dans Réglages,
    Apparence). Un tableau <strong>caché</strong> n'apparaît pas dans la navigation : pratique pour une tablette, à qui on l'impose
    dans Réglages, Écrans.
  </p>
  <ul>
    {#each list as b, k (k)}
      <li>
        <div class="row">
          <button class="ic" onclick={() => (picking = picking === k ? null : k)} aria-label="Icône du tableau"><Icon name={b.icon || 'house'} size={20} /></button>
          <input class="input" bind:value={b.name} aria-label="Nom du tableau" maxlength="40" />
          <button class="mini" onclick={() => move(k, -1)} disabled={k === 0} aria-label="Monter">↑</button>
          <button class="mini" onclick={() => move(k, 1)} disabled={k === list.length - 1} aria-label="Descendre">↓</button>
        </div>
        {#if picking === k}
          <div class="icons">
            {#each ICONS as ic (ic)}
              <button class="ic" class:sel={b.icon === ic} onclick={() => ((b.icon = ic), (picking = null))} aria-label={ic}><Icon name={ic} size={18} /></button>
            {/each}
          </div>
        {/if}
        <div class="row tools">
          <label class="check"><input type="checkbox" bind:checked={b.hidden} /> Caché de la navigation</label>
          <span class="tag">{b.auto ? 'Rangé par pièce' : `${b.sections.reduce((n, s) => n + (s.cards?.length ?? 0), 0)} cartes`}</span>
          {#if b.id && !inline}<button class="link" onclick={() => (showBoard(b.id), onclose?.())}>Afficher</button>{/if}
          <button class="link" onclick={() => duplicate(k)}>Dupliquer</button>
          <button class="link danger" onclick={() => remove(k)} disabled={list.length === 1}>Supprimer</button>
        </div>
      </li>
    {/each}
  </ul>
  <div class="adds">
    <button class="btn quiet" onclick={() => add('empty')}><Icon name="plus" size={16} /> Tableau vide</button>
    <button class="btn quiet" onclick={() => add('auto')}><Icon name="house" size={16} /> Rangé par pièce</button>
  </div>
  <div class="actions">
    <button class="btn" onclick={save} disabled={saving || !dirty}>{saving ? 'Enregistrement…' : 'Enregistrer'}</button>
    {#if dirty}<button class="btn quiet" onclick={() => (list = $state.snapshot(home.boards?.boards ?? []))}>Annuler</button>{/if}
  </div>
{/snippet}

{#if inline}
  {@render body()}
{:else}
  <Sheet title="Tableaux de bord" onclose={() => onclose?.()}>
    {@render body()}
  </Sheet>
{/if}

<style>
  .small {
    font-size: 14px;
  }

  ul {
    list-style: none;
    padding: 0;
    margin: 12px 0;
    display: grid;
    gap: 10px;
  }

  li {
    display: grid;
    gap: 8px;
    padding: 12px;
    border-radius: 16px;
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .row .input {
    flex: 1;
    min-width: 0;
  }

  .tools {
    flex-wrap: wrap;
    gap: 6px 14px;
    font-size: 14px;
  }

  .check {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
  }

  .check input {
    width: 18px;
    height: 18px;
    accent-color: var(--ink);
  }

  .tag {
    color: var(--muted);
  }

  .link {
    color: var(--muted);
    font-weight: 600;
    min-height: 32px;
  }

  .link.danger {
    color: var(--alert);
  }

  .link:disabled {
    opacity: 0.4;
  }

  .ic {
    flex: none;
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    border-radius: 12px;
    background: var(--surface-2);
  }

  .ic.sel {
    background: var(--ink);
    color: var(--bg);
  }

  .icons {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .icons .ic {
    width: 38px;
    height: 38px;
  }

  .mini {
    flex: none;
    width: 36px;
    height: 36px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--surface-2);
    font-weight: 600;
  }

  .mini:disabled {
    opacity: 0.35;
  }

  .adds,
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 8px;
  }
</style>
