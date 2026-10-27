<script>
  import { tr } from '../../lib/i18n.js';
  // Le store des plugins : par catégorie, ceux de Heim (officiels), et l'ajout d'un dépôt
  // externe, avec un avertissement à lire (5 secondes avant de pouvoir installer).
  import { onDestroy, onMount } from 'svelte';
  import Icon from '../Icon.svelte';
  import Sheet from '../Sheet.svelte';
  import { api, toast } from '../../lib/store.svelte.js';

  let { oninstalled = null } = $props();

  let data = $state(null);
  let cat = $state('all');
  let busy = $state(null);
  let source = $state('');
  let preview = $state(null);
  let wait = $state(0);
  let timer;

  const CATS = $derived([
    ['all', tr("Tout")],
    ['connections', tr("Connexions")],
    ['display', tr("Affichage")],
    ['features', tr("Fonctionnalités")],
    ['network', tr("Réseau")],
  ]);
  const PERMS = { network: tr("le réseau de la maison"), usb: tr("les ports USB"), mqtt: tr("le broker MQTT"), bluetooth: tr("le Bluetooth") };

  async function load() {
    data = await api('/plugins').catch(() => data);
  }
  onMount(load);
  onDestroy(() => clearInterval(timer));

  const items = $derived((data?.store ?? []).filter((p) => cat === 'all' || p.category === cat));
  const mem = (mb) => (mb == null ? '' : mb < 1 ? tr("< 1 Mo") : tr("≈ {v} Mo", { v: Math.round(mb) }));

  async function install(body, name) {
    busy = body.id ?? 'external';
    try {
      await api('/plugins/install', { method: 'POST', body });
      toast(tr("{v} installé", { v: name }));
      preview = null;
      await load();
      oninstalled?.();
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = null;
    }
  }

  async function look(e) {
    e.preventDefault();
    busy = 'preview';
    try {
      preview = await api('/plugins/preview', { method: 'POST', body: { source: source.trim() } });
      wait = 5;
      clearInterval(timer);
      timer = setInterval(() => {
        wait = Math.max(0, wait - 1);
        if (!wait) clearInterval(timer);
      }, 1000);
    } catch (err) {
      toast(err.message, 'alert');
    } finally {
      busy = null;
    }
  }
</script>

<div class="cats" role="tablist">
  {#each CATS as [id, label] (id)}
    <button role="tab" aria-selected={cat === id} class:on={cat === id} onclick={() => (cat = id)}>{label}</button>
  {/each}
</div>

{#if !data}
  <p class="muted">{tr("Chargement…")}</p>
{:else}
  <div class="list">
    {#each items as p (p.id)}
      <article class="card">
        <span class="ic"><Icon name={p.icon} size={24} /></span>
        <span class="txt">
          <span class="top"><strong>{p.name}</strong><span class="badge official"><Icon name="check" size={12} /> {tr("officiel")}</span></span>
          <span class="desc">{p.description}</span>
          <span class="meta">
            {p.author ?? 'Heim'}{p.builtin ? ` · ${tr("intégré à Heim")}` : p.version ? ` · v${p.version}` : ''}{p.memory_mb != null ? ` · ${mem(p.memory_mb)}` : ''}
          </span>
          {#if p.memory_note}<span class="meta small">{p.memory_note}</span>{/if}
        </span>
        {#if p.installed}
          <span class="done"><Icon name="check" size={16} /> {tr("Installé")}</span>
        {:else}
          <button class="btn small" disabled={busy || p.available === false} onclick={() => install({ id: p.id }, p.name)}>{busy === p.id ? tr("Installation…") : tr("Installer")}</button>
        {/if}
      </article>
    {:else}
      <p class="muted">{tr("Rien dans cette catégorie pour l'instant. Les plugins de la communauté s'ajoutent par leur dépôt, ci-dessous.")}</p>
    {/each}
  </div>

  <section class="external">
    <h3>{tr("Ajouter un plugin externe")}</h3>
    <p class="muted small">{tr("Le dépôt GitHub d'un plugin (proprietaire/nom), ou l'adresse de son heim-plugin.json.")}</p>
    <form class="add" onsubmit={look}>
      <input class="input" bind:value={source} placeholder="alex/heim-wled" autocomplete="off" autocapitalize="off" spellcheck="false" />
      <button class="btn quiet" type="submit" disabled={!source.trim() || busy}>{busy === 'preview' ? tr("Lecture…") : tr("Voir")}</button>
    </form>
  </section>
{/if}

{#if preview}
  <Sheet title={tr("Plugin externe")} onclose={() => (preview = null)}>
    <div class="head">
      <span class="ic"><Icon name={preview.icon} size={26} /></span>
      <span class="txt">
        <strong>{preview.name}</strong>
        <span class="meta">{preview.author ?? tr("auteur inconnu")}{preview.version ? ` · v${preview.version}` : ''}</span>
      </span>
    </div>
    <p>{preview.description}</p>

    <div class="warn">
      <p><Icon name="alert" size={18} /> <strong>{tr("Développeur non certifié")}</strong></p>
      <p>{tr("Ce plugin ne vient pas du store officiel : personne ne l'a vérifié. Il tournera sur votre box avec les mêmes droits que Heim, donc avec accès à vos appareils et à vos réglages. N'installez que ce qui vient de quelqu'un de confiance.")}</p>
    </div>
    <div class="warn soft">
      <p><Icon name="chip" size={18} /> <strong>{tr("Performances")}</strong></p>
      <p>
        {#if preview.memory_mb != null}{tr("Il annonce environ {v} Mo de mémoire.", { v: Math.round(preview.memory_mb) })}{:else}{tr("Il n'annonce pas sa consommation de mémoire.")}{/if}
        {tr("Surveillez Réglages, Système, Performances après l'installation : sur un Pi Zero 2 W, la mémoire part vite.")}
      </p>
    </div>
    {#if preview.permissions?.length}
      <p class="muted small">{tr("Il demande l'accès à : {v}.", { v: preview.permissions.map((x) => PERMS[x] ?? x).join(', ') })}</p>
    {/if}
    <p class="muted small src">{preview.source}</p>

    <div class="actions">
      <button class="btn quiet" onclick={() => (preview = null)}>{tr("Annuler")}</button>
      <button class="btn danger" disabled={wait > 0 || busy || preview.installed} onclick={() => install({ source: preview.source }, preview.name)}>
        {preview.installed ? tr("Déjà installé") : wait > 0 ? tr("Lisez d'abord… {n}", { n: wait }) : busy ? tr("Installation…") : tr("Installer quand même")}
      </button>
    </div>
  </Sheet>
{/if}

<style>
  .cats {
    display: flex;
    gap: 8px;
    overflow-x: auto;
    padding-bottom: 4px;
    margin-bottom: 12px;
  }

  .cats button {
    flex: none;
    padding: 8px 14px;
    border-radius: var(--radius-chip);
    background: var(--surface-2);
    color: var(--muted);
    font-weight: 600;
    font-size: 14px;
  }

  .cats button.on {
    background: var(--ink);
    color: var(--bg);
  }

  .list {
    display: grid;
    gap: 12px;
  }

  .card,
  .head {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .card {
    padding: 14px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .ic {
    display: grid;
    place-items: center;
    flex: none;
    width: 48px;
    height: 48px;
    border-radius: 14px;
    background: var(--surface-2);
  }

  .txt {
    flex: 1;
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  .top {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .desc,
  .meta {
    font-size: 14px;
    color: var(--muted);
  }

  .small {
    font-size: 13px;
  }

  .badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 1px 8px;
    border-radius: var(--radius-chip);
    font-size: 12px;
    font-weight: 600;
  }

  .badge.official {
    color: var(--moss);
    background: color-mix(in srgb, var(--moss) 14%, transparent);
  }

  .done {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--moss);
    font-weight: 600;
    font-size: 14px;
    white-space: nowrap;
  }

  .external {
    margin-top: 24px;
  }

  h3 {
    margin: 0 0 4px;
    font-size: 16px;
  }

  .add {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }

  .add .input {
    flex: 1;
    min-width: 0;
  }

  .warn {
    margin: 12px 0;
    padding: 12px 14px;
    border-radius: 14px;
    background: color-mix(in srgb, var(--alert) 12%, var(--surface));
  }

  .warn.soft {
    background: color-mix(in srgb, var(--ember) 16%, var(--surface));
  }

  .warn p {
    margin: 0 0 4px;
    display: flex;
    align-items: flex-start;
    gap: 6px;
  }

  .warn p:first-child {
    color: var(--alert);
  }

  .warn.soft p:first-child {
    color: var(--ink);
  }

  .src {
    word-break: break-all;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
    flex-wrap: wrap;
  }

  .btn.danger:not(:disabled) {
    background: var(--alert);
    color: #fff;
  }
</style>
