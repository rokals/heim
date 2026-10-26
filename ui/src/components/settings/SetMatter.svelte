<script>
  import { tr } from '../../lib/i18n.js';
  // Matter par un serveur Matter (celui de Home Assistant, ou python-matter-server).
  import { onMount } from 'svelte';
  import { api, home, toast } from '../../lib/store.svelte.js';

  let { embedded = false } = $props();

  let cfg = $state(null);
  let url = $state('');
  let code = $state('');
  let busy = $state(false);
  let pairing = $state(false);

  async function load() {
    cfg = await api('/matter').catch(() => null);
    url = cfg?.url ?? '';
  }

  onMount(load);

  async function save(enabled) {
    busy = true;
    try {
      await api('/matter', { method: 'PUT', body: { enabled, url: url.trim() } });
      toast(enabled ? tr("Serveur Matter connecté") : tr("Matter désactivé"));
      await load();
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  async function commission(e) {
    e.preventDefault();
    pairing = true;
    try {
      await api('/matter/commission', { method: 'POST', body: { code } });
      toast(tr("Appareil ajouté"));
      code = '';
      await load();
    } catch (err) {
      toast(err.message, 'alert');
    } finally {
      pairing = false;
    }
  }

  async function remove(n) {
    if (!confirm(tr("Retirer « {v} » du réseau Matter ? Il faudra le réappairer.", { v: n.name ?? n.node_id }))) return;
    try {
      await api('/matter/remove', { method: 'POST', body: { node_id: n.node_id } });
      await load();
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  const status = $derived(home.integrations.matter);
</script>

<p class="muted">{tr("Heim ne parle pas Matter lui-même : il se branche sur un")} <strong>{tr("serveur Matter")}</strong>{tr(", celui qu'utilise Home Assistant (module « Matter Server »). Les appareils Matter de ce serveur apparaissent dans Heim.")}</p>

{#if cfg}
  <form
    class="url"
    onsubmit={(e) => {
      e.preventDefault();
      save(true);
    }}
  >
    <label class="field">
      <span>{tr("Adresse du serveur Matter")}</span>
      <input class="input" bind:value={url} placeholder="192.168.1.50:5580" autocomplete="off" autocapitalize="off" inputmode="url" />
    </label>
    <div class="row">
      <button class="btn" type="submit" disabled={busy || !url.trim()}>{busy ? tr("Connexion…") : cfg.enabled ? tr("Enregistrer") : tr("Connecter")}</button>
      {#if cfg.enabled && !embedded}<button class="btn quiet" type="button" disabled={busy} onclick={() => save(false)}>{tr("Désactiver")}</button>{/if}
    </div>
  </form>

  {#if cfg.error}<p class="error">{cfg.error}</p>{/if}
  {#if cfg.server}
    <p class="ok">{tr("Connecté{v}{v2}{v3}", { v: status?.detail ? ` · ${status.detail}` : tr("· SDK {v}", { v: cfg.server.sdk_version ?? '?' }), v2: cfg.server.thread_credentials_set ? tr(" · Thread prêt") : '', v3: cfg.server.bluetooth_enabled ? tr(" · Bluetooth") : '' })}</p>
  {/if}

  {#if cfg.nodes}
    <h2>{tr("Appareils Matter")}</h2>
    {#if !cfg.nodes.length}
      <p class="muted">{tr("Aucun pour l'instant.")}</p>
    {:else}
      <ul class="rows">
        {#each cfg.nodes as n (n.node_id)}
          <li>
            <span class="dot" class:ok={n.available !== false}></span>
            <div class="txt"><strong>{n.name ?? `Nœud ${n.node_id}`}</strong><span>{tr("nœud {node_id} · {entities} entité(s){v}", { node_id: n.node_id, entities: n.entities, v: n.available === false ? tr(" · injoignable") : '' })}</span></div>
            <button class="btn danger" onclick={() => remove(n)}>{tr("Retirer")}</button>
          </li>
        {/each}
      </ul>
    {/if}

    <h2>{tr("Ajouter un appareil")}</h2>
    <p class="muted">{tr("Pour un appareil déjà dans Apple Maison, Google Home, Alexa ou Home Assistant : demandez-y un")} <em>{tr("code d'appairage")}</em> {tr("(« Partager », « Autoriser une autre application »), puis collez-le ici.")}</p>
    <form class="code" onsubmit={commission}>
      <input class="input" bind:value={code} placeholder={tr("MT:… ou 1234-567-8901")} autocomplete="off" autocapitalize="off" />
      <button class="btn" type="submit" disabled={pairing || !code.trim()}>{pairing ? tr("Appairage… (jusqu’à 2 min)") : tr("Appairer")}</button>
    </form>
  {/if}

  <h2>{tr("Le serveur Matter de Home Assistant")}</h2>
  <ol class="steps">
    <li>{tr("Dans Home Assistant : Paramètres > Modules complémentaires >")} <em>{tr("Matter Server")}</em> {tr("> Configuration.")}</li>
    <li>{tr("Dans « Réseau », donnez un port au")} <em>{tr("5580/tcp")}</em> {tr("(même numéro), puis redémarrez le module.")}</li>
    <li>{tr("Indiquez ici l'adresse de Home Assistant (par exemple")} <code>192.168.1.50:5580</code>).</li>
  </ol>
  <p class="muted small">{tr("Les appareils Matter « over Thread » (IKEA récents…) ont besoin d'un routeur Thread (HomePod mini, Nest Hub, clé SkyConnect…) relié au serveur. Un appareil tout neuf s'appaire d'abord dans Home Assistant (qui sait utiliser le Bluetooth du téléphone), puis se partage avec Heim.")}</p>
{/if}

<style>
  h2 {
    margin-top: 28px !important;
  }

  .url,
  .code {
    display: grid;
    gap: 10px;
    max-width: 460px;
  }

  .url .field {
    margin: 0;
  }

  .row {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
  }

  .error {
    color: var(--alert);
    font-weight: 600;
  }

  .ok {
    color: var(--moss);
    font-weight: 600;
  }

  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .rows li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
  }

  .dot {
    flex: none;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--alert);
  }

  .dot.ok {
    background: var(--moss);
  }

  .txt {
    flex: 1;
    display: grid;
    min-width: 0;
  }

  .txt span {
    font-size: 14px;
    color: var(--muted);
  }

  .steps {
    padding-left: 22px;
    display: grid;
    gap: 8px;
  }

  code {
    font-size: 14px;
    padding: 1px 5px;
    border-radius: 5px;
    background: var(--surface-2);
  }

  .small {
    font-size: 14px;
  }
</style>
