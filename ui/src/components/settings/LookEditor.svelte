<script>
  import { tr, LANGS, currentLang } from '../../lib/i18n.js';
  // Les choix d'apparence, pour soi, pour la maison ou pour un écran.
  // look : les choix faits (une clé absente = « comme au-dessus ») ; onchange(nouveaux choix).
  import Icon from '../Icon.svelte';
  import { ACCENTS, DEFAULT_LOOK } from '../../lib/theme.js';

  /** lang : proposer aussi la langue (pas dans l'assistant, qui la choisit à part). */
  let { look = {}, base = DEFAULT_LOOK, onchange, nav = true, lang = true } = $props();

  const val = (k) => look?.[k] ?? base[k];
  const custom = $derived(val('accent') && !ACCENTS.some((a) => a.id === val('accent')));

  function set(k, v) {
    onchange({ ...look, [k]: v });
  }

  const GROUPS = [
    { key: 'theme', label: tr("Thème"), options: [['system', tr("Comme l’appareil")], ['light', tr("Clair")], ['dark', tr("Sombre")]] },
    { key: 'text', label: tr("Taille du texte"), options: [['normal', tr("Normale")], ['large', tr("Grande")]] },
    { key: 'radius', label: tr("Arrondi des tuiles"), options: [['square', tr("Carré")], ['normal', tr("Normal")], ['round', tr("Très rond")]] },
    { key: 'density', label: tr("Espacement"), options: [['compact', tr("Compact")], ['normal', tr("Normal")], ['airy', tr("Aéré")]] },
    { key: 'background', label: tr("Fond"), options: [['plain', tr("Uni")], ['gradient', tr("Léger dégradé")]] },
  ];
</script>

{#if lang}
  <div class="field">
    <span>{tr('Langue')}</span>
    <div class="seg wrap">
      {#each LANGS as l (l.id)}
        <button class:on={(look?.lang ?? base.lang ?? currentLang()) === l.id} onclick={() => set('lang', l.id)} lang={l.id}>{l.label}</button>
      {/each}
    </div>
  </div>
{/if}

<div class="field">
  <span>{tr("Couleur de l'interface")}</span>
  <div class="accents" role="radiogroup" aria-label={tr("Couleur de l'interface")}>
    {#each ACCENTS as a (a.id)}
      <button
        class="sw"
        class:sel={val('accent') === a.id}
        style:background={a.hex ?? a.id}
        onclick={() => set('accent', a.id)}
        role="radio"
        aria-checked={val('accent') === a.id}
        aria-label={a.label}
        title={a.label}
      >{#if val('accent') === a.id}<Icon name="check" size={16} />{/if}</button>
    {/each}
    <label class="sw pick" class:sel={custom} title={tr("Autre couleur")} style:background={custom ? val('accent') : null}>
      <input type="color" value={custom ? val('accent') : '#f0a93a'} onchange={(e) => set('accent', e.currentTarget.value)} aria-label={tr("Autre couleur")} />
      {#if !custom}<Icon name="plus" size={16} />{/if}
    </label>
  </div>
</div>

{#each GROUPS as g (g.key)}
  <div class="field">
    <span>{g.label}</span>
    <div class="seg wrap">
      {#each g.options as [id, label] (id)}
        <button class:on={val(g.key) === id} onclick={() => set(g.key, id)}>{label}</button>
      {/each}
    </div>
  </div>
{/each}

{#if nav}
  <div class="field">
    <span>{tr("Changer de tableau")}</span>
    <div class="seg wrap">
      <button class:on={val('nav') === 'swipe'} onclick={() => set('nav', 'swipe')}>{tr("En glissant (petits points)")}</button>
      <button class:on={val('nav') === 'tabs'} onclick={() => set('nav', 'tabs')}>{tr("Par onglets")}</button>
    </div>
  </div>
{/if}

<style>
  .accents {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .sw {
    position: relative;
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    border-radius: 50%;
    border: 2px solid transparent;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.12);
    color: #fff;
    cursor: pointer;
  }

  .sw.sel {
    border-color: var(--ink);
    transform: scale(1.06);
  }

  .pick {
    background: conic-gradient(#e5484d, #ffc53d, #46a758, #0090ff, #8e4ec6, #e5484d);
    overflow: hidden;
  }

  .pick input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
  }

  .seg {
    display: flex;
    gap: 6px;
  }

  .seg.wrap {
    flex-wrap: wrap;
  }

  .seg button {
    min-height: 42px;
    padding: 0 14px;
    border-radius: var(--radius-chip);
    border: 1.5px solid var(--line);
    font-weight: 600;
  }

  .seg button.on {
    background: var(--ink);
    border-color: var(--ink);
    color: var(--bg);
  }
</style>
