<script>
  import { tr } from '../../lib/i18n.js';
  // Apparence : thème, couleur, texte, arrondi, espacement, fond, façon de changer de tableau.
  // Chacun règle la sienne ; l'admin règle aussi celle de la maison (pour tous par défaut).
  import LookEditor from './LookEditor.svelte';
  import { api, home, isAdmin, saveMyLook, toast } from '../../lib/store.svelte.js';
  import { cleanLook, effectiveLook } from '../../lib/theme.js';

  const hasAccount = $derived(Boolean(home.me?.user));
  let scope = $state('mine');
  const which = $derived(hasAccount ? scope : 'house');

  async function saveHouse(look) {
    const clean = cleanLook(look);
    home.look.house = clean;
    try {
      await api('/appearance', { method: 'PUT', body: clean });
    } catch (e) {
      toast(e.message, 'alert');
    }
  }
</script>

{#if hasAccount && isAdmin()}
  <div class="tabs" role="tablist">
    <button role="tab" aria-selected={scope === 'mine'} class:on={scope === 'mine'} onclick={() => (scope = 'mine')}>{tr("Pour moi")}</button>
    <button role="tab" aria-selected={scope === 'house'} class:on={scope === 'house'} onclick={() => (scope = 'house')}>{tr("Pour toute la maison")}</button>
  </div>
{/if}

{#if which === 'mine'}
  <p class="muted">
    {tr("Votre apparence vous suit sur tous vos appareils.")}
    {#if home.look?.mine}<button class="link" onclick={() => saveMyLook(null)}>{tr("Reprendre celle de la maison")}</button>{:else}{tr("Pour l’instant, c’est celle de la maison.")}{/if}
  </p>
  <LookEditor
    look={home.look?.mine ?? {}}
    base={effectiveLook(home.look?.house)}
    onchange={(l) => saveMyLook(cleanLook(l))}
  />
{:else}
  <p class="muted">{tr("Celle de tout le monde, tant que chacun n’a pas choisi la sienne.")}</p>
  <LookEditor look={home.look?.house ?? {}} onchange={saveHouse} />
{/if}

{#if home.screen?.appearance}
  <p class="note">{tr("Cet écran a sa propre apparence, réglée dans Réglages, Écrans : elle passe avant.")}</p>
{/if}

<style>
  .tabs {
    display: flex;
    padding: 3px;
    border-radius: 14px;
    background: var(--surface-2);
    margin-bottom: 12px;
  }

  .tabs button {
    flex: 1;
    min-height: 40px;
    border-radius: 11px;
    font-weight: 600;
    color: var(--muted);
  }

  .tabs button.on {
    background: var(--surface);
    color: var(--ink);
    box-shadow: var(--shadow);
  }

  .link {
    color: var(--focus);
    font-weight: 600;
    text-decoration: underline;
  }

  .note {
    margin-top: 18px;
    padding: 12px 14px;
    border-radius: 14px;
    background: var(--surface-2);
    font-size: 14px;
  }
</style>
