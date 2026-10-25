<script>
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
      toast(enabled ? 'Serveur Matter connecté' : 'Matter désactivé');
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
      toast('Appareil ajouté');
      code = '';
      await load();
    } catch (err) {
      toast(err.message, 'alert');
    } finally {
      pairing = false;
    }
  }

  async function remove(n) {
    if (!confirm(`Retirer « ${n.name ?? n.node_id} » du réseau Matter ? Il faudra le réappairer.`)) return;
    try {
      await api('/matter/remove', { method: 'POST', body: { node_id: n.node_id } });
      await load();
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  const status = $derived(home.integrations.matter);
</script>

<p class="muted">Heim ne parle pas Matter lui-même : il se branche sur un <strong>serveur Matter</strong>, celui qu'utilise Home Assistant (module « Matter Server »). Les appareils Matter de ce serveur apparaissent dans Heim.</p>

{#if cfg}
  <form
    class="url"
    onsubmit={(e) => {
      e.preventDefault();
      save(true);
    }}
  >
    <label class="field">
      <span>Adresse du serveur Matter</span>
      <input class="input" bind:value={url} placeholder="192.168.1.50:5580" autocomplete="off" autocapitalize="off" inputmode="url" />
    </label>
    <div class="row">
      <button class="btn" type="submit" disabled={busy || !url.trim()}>{busy ? 'Connexion…' : cfg.enabled ? 'Enregistrer' : 'Connecter'}</button>
      {#if cfg.enabled && !embedded}<button class="btn quiet" type="button" disabled={busy} onclick={() => save(false)}>Désactiver</button>{/if}
    </div>
  </form>

  {#if cfg.error}<p class="error">{cfg.error}</p>{/if}
  {#if cfg.server}
    <p class="ok">Connecté{status?.detail ? ` · ${status.detail}` : ` · SDK ${cfg.server.sdk_version ?? '?'}`}{cfg.server.thread_credentials_set ? ' · Thread prêt' : ''}{cfg.server.bluetooth_enabled ? ' · Bluetooth' : ''}</p>
  {/if}

  {#if cfg.nodes}
    <h2>Appareils Matter</h2>
    {#if !cfg.nodes.length}
      <p class="muted">Aucun pour l'instant.</p>
    {:else}
      <ul class="rows">
        {#each cfg.nodes as n (n.node_id)}
          <li>
            <span class="dot" class:ok={n.available !== false}></span>
            <div class="txt"><strong>{n.name ?? `Nœud ${n.node_id}`}</strong><span>nœud {n.node_id} · {n.entities} entité(s){n.available === false ? ' · injoignable' : ''}</span></div>
            <button class="btn danger" onclick={() => remove(n)}>Retirer</button>
          </li>
        {/each}
      </ul>
    {/if}

    <h2>Ajouter un appareil</h2>
    <p class="muted">Pour un appareil déjà dans Apple Maison, Google Home, Alexa ou Home Assistant : demandez-y un <em>code d'appairage</em> (« Partager », « Autoriser une autre application »), puis collez-le ici.</p>
    <form class="code" onsubmit={commission}>
      <input class="input" bind:value={code} placeholder="MT:… ou 1234-567-8901" autocomplete="off" autocapitalize="off" />
      <button class="btn" type="submit" disabled={pairing || !code.trim()}>{pairing ? 'Appairage… (jusqu’à 2 min)' : 'Appairer'}</button>
    </form>
  {/if}

  <h2>Le serveur Matter de Home Assistant</h2>
  <ol class="steps">
    <li>Dans Home Assistant : Paramètres &gt; Modules complémentaires &gt; <em>Matter Server</em> &gt; Configuration.</li>
    <li>Dans « Réseau », donnez un port au <em>5580/tcp</em> (même numéro), puis redémarrez le module.</li>
    <li>Indiquez ici l'adresse de Home Assistant (par exemple <code>192.168.1.50:5580</code>).</li>
  </ol>
  <p class="muted small">Les appareils Matter « over Thread » (IKEA récents…) ont besoin d'un routeur Thread (HomePod mini, Nest Hub, clé SkyConnect…) relié au serveur. Un appareil tout neuf s'appaire d'abord dans Home Assistant (qui sait utiliser le Bluetooth du téléphone), puis se partage avec Heim.</p>
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
