<script>
  import { tr, locale } from '../../lib/i18n.js';
  // Un plugin externe : état, mémoire, réglages (formulaire tiré de son manifeste),
  // journal, relance, désinstallation.
  import { onDestroy, onMount } from 'svelte';
  import Icon from '../Icon.svelte';
  import { api, toast } from '../../lib/store.svelte.js';

  let { id, onremoved = null } = $props();

  let p = $state(null);
  let form = $state({});
  let log = $state([]);
  let busy = $state(false);
  let timer;

  async function load(first = false) {
    const all = await api('/plugins').catch(() => null);
    const found = all?.installed?.find((x) => x.id === id);
    if (found) {
      p = found;
      if (first) form = { ...found.settings };
    }
    log = (await api(`/plugins/${id}/log`).catch(() => ({ lines: [] }))).lines;
  }
  onMount(() => {
    load(true);
    timer = setInterval(load, 4000);
  });
  onDestroy(() => clearInterval(timer));

  async function save(e) {
    e.preventDefault();
    busy = true;
    try {
      p = await api(`/plugins/${id}`, { method: 'PUT', body: { settings: $state.snapshot(form), enabled: true } });
      toast(tr("Réglages enregistrés, le plugin redémarre"));
    } catch (err) {
      toast(err.message, 'alert');
    } finally {
      busy = false;
    }
  }

  async function restart() {
    await api(`/plugins/${id}/restart`, { method: 'POST' }).catch((e) => toast(e.message, 'alert'));
    toast(tr("Plugin relancé"));
  }

  async function remove() {
    if (!confirmRemove) {
      confirmRemove = true;
      return;
    }
    busy = true;
    try {
      await api(`/plugins/${id}`, { method: 'DELETE' });
      toast(tr("Plugin désinstallé"));
      onremoved?.();
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }
  let confirmRemove = $state(false);

  const mo = (b) => tr("{v} Mo", { v: Math.round(b / 1048576) });
  const since = (iso) => new Date(iso).toLocaleString(locale(), { hour: '2-digit', minute: '2-digit', day: 'numeric', month: 'short' });
</script>

{#if p}
  <dl class="facts">
    <div><dt>{tr("Version")}</dt><dd>{p.version ?? '–'}</dd></div>
    <div><dt>{tr("Auteur")}</dt><dd>{p.author ?? '–'}{p.official ? '' : ` · ${tr("communauté")}`}</dd></div>
    <div><dt>{tr("Mémoire")}</dt><dd class="num">{p.usage?.memory ? mo(p.usage.memory) : '–'}{p.memory_mb != null ? ` (${tr("annoncé : {v} Mo", { v: Math.round(p.memory_mb) })})` : ''}</dd></div>
    <div><dt>{tr("Processeur")}</dt><dd class="num">{p.usage?.cpu != null ? `${p.usage.cpu} %` : '–'}</dd></div>
    <div><dt>{tr("Démarré")}</dt><dd>{p.run?.running && p.run.started_at ? since(p.run.started_at) : tr("arrêté")}</dd></div>
    {#if p.run?.restarts}<div><dt>{tr("Redémarrages")}</dt><dd class="num">{p.run.restarts}</dd></div>{/if}
  </dl>

  {#if p.settings_schema?.length}
    <h3>{tr("Réglages")}</h3>
    <form onsubmit={save}>
      {#each p.settings_schema as d (d.key)}
        {#if d.type === 'boolean'}
          <label class="check"><input type="checkbox" bind:checked={form[d.key]} /> {d.label || d.key}</label>
        {:else}
          <label class="field">
            <span>{d.label || d.key}{d.required ? ' *' : ''}</span>
            {#if d.type === 'select'}
              <select class="input" bind:value={form[d.key]}>
                {#each d.options as o (String(o))}<option value={o}>{o}</option>{/each}
              </select>
            {:else}
              <input class="input" type={d.type === 'password' ? 'password' : d.type === 'number' ? 'number' : 'text'} step="any" bind:value={form[d.key]} placeholder={d.default != null ? String(d.default) : ''} autocomplete="off" />
            {/if}
            {#if d.help}<small class="muted">{d.help}</small>{/if}
          </label>
        {/if}
      {/each}
      <button class="btn" type="submit" disabled={busy}>{tr("Enregistrer et démarrer")}</button>
    </form>
  {/if}

  <div class="actions">
    <button class="btn quiet" onclick={restart} disabled={busy || !p.enabled}><Icon name="restart" size={18} /> {tr("Relancer")}</button>
    <button class="btn quiet danger" onclick={remove} disabled={busy}>{confirmRemove ? tr("Confirmer la désinstallation") : tr("Désinstaller")}</button>
  </div>

  <details open={p.run?.last_exit && !p.run?.running}>
    <summary>{tr("Journal du plugin")}</summary>
    {#if log.length}<pre>{log.join('\n')}</pre>{:else}<p class="muted">{tr("Rien pour l'instant.")}</p>{/if}
  </details>
{:else}
  <p class="muted">{tr("Chargement…")}</p>
{/if}

<style>
  .facts {
    display: grid;
    gap: 8px;
    margin: 8px 0 16px;
  }

  .facts div {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }

  dt {
    color: var(--muted);
  }

  dd {
    margin: 0;
    text-align: right;
  }

  h3 {
    margin: 20px 0 8px;
    font-size: 16px;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 8px 0;
  }

  .check input {
    width: 20px;
    height: 20px;
    accent-color: var(--ink);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 16px 0;
  }

  .danger {
    color: var(--alert);
  }

  summary {
    cursor: pointer;
    color: var(--muted);
    font-weight: 600;
  }

  pre {
    max-height: 260px;
    overflow: auto;
    padding: 10px;
    border-radius: 12px;
    background: var(--surface-2);
    font-size: 11px;
    white-space: pre-wrap;
    word-break: break-word;
  }
</style>
