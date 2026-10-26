<script>
  import { tr } from '../lib/i18n.js';
  import Icon from './Icon.svelte';
  import AutomationEditor from './AutomationEditor.svelte';
  import { describeTrigger, PRESETS } from '../lib/automation.js';
  import { ago } from '../lib/house.js';
  import { api, home, isAdmin, loadAutomations, toast } from '../lib/store.svelte.js';

  let { page = '', onnavigate = null } = $props();
  let editing = $state(null);

  // #automatismes/<id> (depuis un bouton de l'accueil) : ouvre directement l'automatisme.
  $effect(() => {
    if (!page) return;
    const a = home.automations.find((x) => x.id === page);
    if (a && isAdmin()) {
      editing = { automation: a };
      onnavigate?.('');
    }
  });

  async function toggle(a) {
    try {
      await api(`/automations/${a.id}/op`, { method: 'POST', body: { enabled: !a.enabled } });
      await loadAutomations();
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  async function rearm(a) {
    try {
      await api(`/automations/${a.id}/op`, { method: 'POST', body: { state: 'rearm' } });
      toast(tr("{name} : réarmé", { name: a.name }));
      await loadAutomations();
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  function summary(a) {
    const when = a.triggers.map((t) => describeTrigger(t, home.entities)).join(' ou ');
    return when ? tr("Quand {when}", { when }) : '';
  }
</script>

<header class="top">
  <h1>{tr("Automatismes")}</h1>
  {#if isAdmin()}<button class="btn" onclick={() => (editing = { automation: null })}><Icon name="plus" size={18} /> {tr("Nouvel automatisme")}</button>{/if}
</header>
<p class="intro">{tr("Quand quelque chose se passe, et si les conditions sont réunies, Heim agit tout seul.")}</p>

{#if !home.automations.length && isAdmin()}
  <h2>{tr("Pour commencer")}</h2>
  <div class="presets">
    {#each PRESETS as p (p.title)}
      <button class="preset" onclick={() => (editing = { preset: structuredClone(p.value) })}>
        <strong>{p.title}</strong>
        <span>{p.detail}</span>
      </button>
    {/each}
  </div>
{/if}

<ul class="list">
  {#each home.automations as a (a.id)}
    <li class:off={!a.enabled}>
      <button class="main" onclick={() => isAdmin() && (editing = { automation: a })}>
        <strong>
          {a.name}
          <span class="flag" class:up={a.state !== a.rest_state}>{a.state ? tr("Vrai") : tr("Faux")}</span>
        </strong>
        <span>{summary(a)}</span>
        {#if a.last_error}
          <span class="err">{tr("Dernier essai : {last_error}", { last_error: a.last_error })}</span>
        {:else if a.last_check && !a.last_check.ran}
          <span class="skip">{tr("Pas lancé {ago} : {reason}", { ago: ago(a.last_check.at, home.now), reason: a.last_check.reason })}</span>
        {:else if a.last_run}
          <span class="ran">{tr("Exécuté {ago}", { ago: ago(a.last_run, home.now) })}</span>
        {/if}
      </button>
      {#if a.state !== a.rest_state}
        <button class="btn quiet rearm" onclick={() => rearm(a)}>{tr("Réarmer")}</button>
      {/if}
      <label class="switch" title={a.enabled ? tr("Actif") : tr("En pause")}>
        <input type="checkbox" checked={a.enabled} disabled={!isAdmin()} onchange={() => toggle(a)} aria-label={tr("Activer {name}", { name: a.name })} />
        <span></span>
      </label>
    </li>
  {/each}
</ul>

{#if editing}
  <AutomationEditor automation={editing.automation ?? null} preset={editing.preset ?? null} onclose={() => (editing = null)} />
{/if}

<style>
  .flag {
    display: inline-block;
    margin-left: 6px;
    padding: 1px 8px;
    border-radius: 9px;
    font-size: 12px;
    font-weight: 600;
    vertical-align: middle;
    background: var(--surface-2);
    color: var(--muted);
  }

  .flag.up {
    background: #e5484d;
    color: #fff;
  }

  .skip {
    color: var(--muted);
    font-style: italic;
  }

  .rearm {
    flex: none;
    min-height: 38px;
  }

  .top {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  h1 {
    margin: 0;
    font-size: 40px;
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  h2 {
    font-size: 20px;
    margin: 24px 0 10px;
  }

  .intro {
    color: var(--muted);
    max-width: 60ch;
  }

  .presets {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(260px, 100%), 1fr));
    gap: 10px;
  }

  .preset {
    display: grid;
    gap: 4px;
    padding: 16px;
    border-radius: var(--radius-tile);
    border: 1.5px dashed var(--line);
    text-align: left;
  }

  .preset span {
    color: var(--muted);
    font-size: 14px;
  }

  .list {
    list-style: none;
    padding: 0;
    margin: 20px 0 0;
    display: grid;
    gap: 8px;
  }

  li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-right: 14px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  li.off {
    opacity: 0.6;
  }

  .main {
    flex: 1;
    display: grid;
    gap: 2px;
    padding: 14px 16px;
    text-align: left;
    min-width: 0;
  }

  .main span {
    font-size: 14px;
    color: var(--muted);
  }

  .main .err {
    color: var(--alert);
  }

  .main .ran {
    color: var(--moss);
  }

  .switch {
    position: relative;
    flex: none;
    width: 52px;
    height: 32px;
  }

  .switch input {
    position: absolute;
    inset: 0;
    opacity: 0;
    margin: 0;
    cursor: pointer;
  }

  .switch span {
    position: absolute;
    inset: 0;
    border-radius: 16px;
    background: var(--surface-2);
    transition: background-color 0.2s;
    pointer-events: none;
  }

  .switch span::after {
    content: '';
    position: absolute;
    top: 3px;
    left: 3px;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    background: var(--bg);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.2);
    transition: transform 0.2s;
  }

  .switch input:checked + span {
    background: var(--moss);
  }

  .switch input:checked + span::after {
    transform: translateX(20px);
  }

  .switch input:focus-visible + span {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
</style>
