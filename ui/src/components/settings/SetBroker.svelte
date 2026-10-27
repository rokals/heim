<script>
  import { tr } from '../../lib/i18n.js';
  // Le broker MQTT intégré : qui est branché, ouverture au réseau, comptes des appareils.
  import { onDestroy, onMount } from 'svelte';
  import Icon from '../Icon.svelte';
  import { api, toast } from '../../lib/store.svelte.js';
  import { locale } from '../../lib/i18n.js';

  let st = $state(null);
  let busy = $state(false);
  let newUser = $state('');
  let created = $state(null);
  let port = $state(1883);
  let timer;

  async function load(first = false) {
    try {
      st = await api('/broker');
      if (first) port = st.settings.port;
    } catch {
      /* on réessaie */
    }
  }

  onMount(() => {
    load(true);
    timer = setInterval(load, 4000);
  });
  onDestroy(() => clearInterval(timer));

  async function save(patch) {
    busy = true;
    try {
      st = await api('/broker', { method: 'PUT', body: { enabled: st.settings.enabled, open: st.settings.open, port: st.settings.port, ...patch } });
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  async function addUser(e) {
    e.preventDefault();
    busy = true;
    try {
      created = await api('/broker/users', { method: 'POST', body: { username: newUser.trim() } });
      newUser = '';
      await load();
    } catch (err) {
      toast(err.message, 'alert');
    } finally {
      busy = false;
    }
  }

  async function removeUser(name) {
    busy = true;
    try {
      st = await api(`/broker/users/${encodeURIComponent(name)}`, { method: 'DELETE' });
      if (created?.username === name) created = null;
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  async function copy(text) {
    try {
      await navigator.clipboard.writeText(text);
      toast(tr("Copié"));
    } catch {
      /* pas de presse-papiers (http) : le texte reste affiché */
    }
  }

  const since = (iso) => new Date(iso).toLocaleTimeString(locale(), { hour: '2-digit', minute: '2-digit' });
</script>

{#if st}
  <p class="muted">
    {tr("Zigbee2MQTT et les autres appareils MQTT se branchent directement sur Heim : pas besoin de Mosquitto ni de Home Assistant.")}
  </p>

  {#if st.error}
    <p class="warn"><Icon name="alert" size={18} /> {st.error}</p>
  {/if}

  {#if st.settings.enabled}
    <dl class="facts">
      <div><dt>{tr("Adresse")}</dt><dd><code>mqtt://{st.settings.open ? location.hostname : '127.0.0.1'}:{st.settings.port}</code></dd></div>
      <div><dt>{tr("Messages retenus")}</dt><dd class="num">{st.retained}</dd></div>
      <div><dt>{tr("Reçus / envoyés")}</dt><dd class="num">{st.received} / {st.sent}</dd></div>
    </dl>

    <h3>{tr("Branchés en ce moment")}</h3>
    {#if st.clients.length}
      <ul class="rows">
        {#each st.clients as c (c.client_id)}
          <li>
            <span class="txt"><strong>{c.client_id}</strong><small>{c.local ? tr("sur la box") : c.address}{c.username ? ` · ${c.username}` : ''}</small></span>
            <span class="muted small">{tr("depuis {v}", { v: since(c.since) })}</span>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="muted">{tr("Personne pour l'instant.")}</p>
    {/if}

    <h3>{tr("Appareils du réseau")}</h3>
    <label class="check">
      <input type="checkbox" checked={st.settings.open} disabled={busy} onchange={(e) => save({ open: e.currentTarget.checked })} />
      {tr("Ouvrir le broker au réseau de la maison")}
    </label>
    <p class="muted small">
      {tr("Pour des prises Tasmota, Shelly, des montages ESPHome… Chacun se connecte avec un compte. La box elle-même (Zigbee2MQTT) n'en a pas besoin.")}
    </p>

    {#if st.settings.open}
      {#if st.settings.users.length}
        <ul class="rows">
          {#each st.settings.users as u (u.username)}
            <li>
              <span class="txt"><strong>{u.username}</strong></span>
              <button class="mini" onclick={() => removeUser(u.username)} disabled={busy} aria-label={tr("Supprimer {v}", { v: u.username })}><Icon name="close" size={16} /></button>
            </li>
          {/each}
        </ul>
      {/if}
      {#if created}
        <div class="created">
          <p>{tr("Compte « {v} » créé. Notez le mot de passe, il ne sera plus affiché :", { v: created.username })}</p>
          <p class="pw"><code>{created.password}</code> <button class="btn quiet small" onclick={() => copy(created.password)}><Icon name="copy" size={16} /> {tr("Copier")}</button></p>
        </div>
      {/if}
      <form class="add" onsubmit={addUser}>
        <input class="input" bind:value={newUser} placeholder={tr("Nom du compte (ex. : shelly)")} maxlength="64" autocomplete="off" />
        <button class="btn" type="submit" disabled={busy || !newUser.trim()}><Icon name="plus" size={18} /> {tr("Ajouter")}</button>
      </form>
    {/if}

    <details>
      <summary>{tr("Réglages avancés")}</summary>
      <div class="add">
        <label class="field grow">
          <span>{tr("Port")}</span>
          <input class="input num" type="number" min="1" max="65535" bind:value={port} />
        </label>
        <button class="btn quiet" disabled={busy || Number(port) === st.settings.port} onclick={() => save({ port: Number(port) })}>{tr("Enregistrer")}</button>
      </div>
    </details>
  {:else}
    <p class="muted">{tr("Le broker est éteint. Allumez-le avec l'interrupteur ci-dessus, ou démarrez Zigbee sur la box : il s'allume tout seul.")}</p>
  {/if}
{/if}

<style>
  h3 {
    margin: 24px 0 8px;
    font-size: 16px;
  }

  .facts {
    display: grid;
    gap: 8px;
    margin: 12px 0;
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

  .rows {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  .rows li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
  }

  .txt {
    display: grid;
    min-width: 0;
  }

  .txt strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .txt small,
  .small {
    font-size: 13px;
    color: var(--muted);
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

  .add {
    display: flex;
    align-items: flex-end;
    gap: 8px;
    margin: 12px 0;
  }

  .add .input {
    flex: 1;
    min-width: 0;
  }

  .grow {
    flex: 1;
    margin: 0;
  }

  .mini {
    flex: none;
    width: 34px;
    height: 34px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--muted);
  }

  .created {
    margin: 12px 0;
    padding: 12px 14px;
    border-radius: 14px;
    background: color-mix(in srgb, var(--moss) 14%, var(--surface));
  }

  .created p {
    margin: 0 0 6px;
  }

  .pw {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .pw code {
    font-size: 16px;
    letter-spacing: 0.04em;
    word-break: break-all;
  }

  .warn {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--alert);
    font-weight: 600;
  }

  summary {
    cursor: pointer;
    color: var(--muted);
    font-weight: 600;
    min-height: 36px;
    display: flex;
    align-items: center;
  }

  details {
    margin-top: 16px;
  }
</style>
