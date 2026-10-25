<script>
  import Icon from '../Icon.svelte';
  import PushCard from './PushCard.svelte';
  import { api, deviceId, home, isAdmin, rememberDeviceLabel, toast } from '../../lib/store.svelte.js';

  let targets = $state([]);
  let myName = $state('');
  let message = $state('');
  let except = $state('');
  let hookName = $state('');
  let hookUrl = $state('');

  const me = $derived(home.targetId || deviceId());

  async function load() {
    try {
      targets = await api('/notify/targets');
      myName = targets.find((t) => t.id === me)?.name ?? myName;
    } catch {
      targets = [];
    }
  }

  $effect(() => {
    if (home.connected && isAdmin()) load();
  });

  async function renameMe(e) {
    e.preventDefault();
    try {
      await api(`/notify/targets/${me}`, { method: 'PUT', body: { name: myName.trim(), kind: 'app' } });
      rememberDeviceLabel(myName.trim());
      toast('Nom de cet appareil enregistré');
      load();
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  async function send(e) {
    e.preventDefault();
    try {
      const r = await api('/notify', { method: 'POST', body: { message, except: except ? [except] : [] } });
      toast(r.recipients.length === 1 ? 'Envoyée à 1 appareil' : `Envoyée à ${r.recipients.length} appareils`);
      message = '';
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  async function addHook(e) {
    e.preventDefault();
    try {
      await api(`/notify/targets/${encodeURIComponent(hookName.trim())}`, {
        method: 'PUT',
        body: { name: hookName.trim(), kind: 'webhook', url: hookUrl.trim() },
      });
      hookName = '';
      hookUrl = '';
      load();
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  async function forget(t) {
    if (!confirm(`Ne plus envoyer de notifications à « ${t.name} » ?`)) return;
    await api(`/notify/targets/${t.id}`, { method: 'DELETE' }).catch((err) => toast(err.message, 'alert'));
    load();
  }
</script>

<PushCard />

{#if isAdmin()}
<p class="muted">Chaque téléphone ou écran qui ouvre Heim reçoit les notifications. Donnez-lui un nom pour le reconnaître.</p>

<form class="inline" onsubmit={renameMe}>
  <label class="field grow">
    <span>Nom de cet appareil</span>
    <input class="input" bind:value={myName} placeholder="Téléphone de Alex" />
  </label>
  <button class="btn quiet" type="submit" disabled={!myName.trim()}>Renommer</button>
</form>

<h2>Destinataires</h2>
<ul class="rows">
  {#each targets as t (t.id)}
    <li>
      <Icon name={t.kind === 'webhook' ? 'bolt' : 'bell'} size={18} />
      <span class="grow">
        {t.name}{t.id === me ? ' (cet appareil)' : ''}{t.kind === 'webhook' ? ' (service web)' : ''}
        <small>{[t.person ? `compte ${t.person}` : '', t.push ? 'notifications push' : t.kind === 'app' ? 'seulement Heim ouvert' : ''].filter(Boolean).join(' · ')}</small>
      </span>
      <button class="btn danger" onclick={() => forget(t)}>Retirer</button>
    </li>
  {/each}
</ul>

<h2>Message de test</h2>
<form onsubmit={send}>
  <label class="field">
    <span>Message</span>
    <input class="input" bind:value={message} placeholder="Le linge est prêt" />
  </label>
  <label class="field">
    <span>À tout le monde sauf</span>
    <select class="input" bind:value={except}>
      <option value="">personne</option>
      {#each targets as t (t.id)}<option value={t.id}>{t.name}</option>{/each}
    </select>
  </label>
  <button class="btn" type="submit" disabled={!message.trim()}>Envoyer</button>
</form>

<h2>Service web (ntfy, Gotify, Discord…)</h2>
<p class="muted">Pour envoyer aussi les notifications vers un autre service.</p>
<form onsubmit={addHook}>
  <div class="two">
    <label class="field">
      <span>Nom</span>
      <input class="input" bind:value={hookName} placeholder="ntfy" />
    </label>
    <label class="field">
      <span>Adresse</span>
      <input class="input" bind:value={hookUrl} placeholder="https://ntfy.sh/ma-maison" inputmode="url" />
    </label>
  </div>
  <button class="btn quiet" type="submit" disabled={!hookName.trim() || !hookUrl.trim()}>Ajouter</button>
</form>
{/if}

<style>
  .inline {
    display: flex;
    gap: 10px;
    align-items: flex-end;
    flex-wrap: wrap;
  }

  .inline .field {
    margin-bottom: 0;
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .grow small {
    display: block;
    font-size: 13px;
    color: var(--muted);
  }

  h2 {
    margin-top: 28px !important;
  }

  .rows {
    list-style: none;
    padding: 0;
    margin: 0;
  }

  .rows li {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 52px;
    border-bottom: 1px solid var(--line);
  }

  .two {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 0 12px;
  }
</style>
