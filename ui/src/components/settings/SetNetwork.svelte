<script>
  // Wi-Fi du Raspberry Pi (image Heim). Sur un ordinateur, c'est le système qui s'en occupe.
  import { onMount } from 'svelte';
  import { api, toast } from '../../lib/store.svelte.js';

  let st = $state(null);
  let networks = $state([]);
  let ssid = $state('');
  let password = $state('');
  let busy = $state(false);

  async function load() {
    st = await fetch('/api/setup').then((r) => r.json()).catch(() => null);
  }

  onMount(async () => {
    await load();
    if (st?.netsetup) scan();
  });

  async function scan() {
    busy = true;
    networks = await api('/setup/scan', { method: 'POST' }).catch(() => []);
    busy = false;
  }

  async function join(e) {
    e.preventDefault();
    if (!confirm(`Heim va rejoindre « ${ssid} ». S'il n'y arrive pas, il revient sur le réseau actuel, ou ouvre son réseau « ${st.network.hotspot_ssid} ». Continuer ?`)) return;
    try {
      await api('/setup/wifi', { method: 'POST', body: { ssid, password } });
      toast(`Connexion à « ${ssid} »… Si cette page ne répond plus, rejoignez ce Wi-Fi et ouvrez http://heim.local`);
      password = '';
      setTimeout(load, 15000);
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  const MODES = { ethernet: 'Câble réseau', wifi: 'Wi-Fi', hotspot: 'Réseau Heim (configuration)', none: 'Aucune connexion' };
</script>

{#if !st}
  <p class="muted">Chargement…</p>
{:else if !st.netsetup}
  <p class="muted">Ici, c'est le système de la machine qui gère le réseau (Windows, ou un Linux sans l'image Heim). Heim utilise simplement la connexion existante.</p>
  {#if st.local_ip}<p>Adresse de Heim : <strong>{st.local_ip}</strong></p>{/if}
{:else}
  <dl>
    <dt>Connexion</dt><dd>{MODES[st.network.mode] ?? st.network.mode}{st.network.ssid ? ` : ${st.network.ssid}` : ''}</dd>
    <dt>Adresse</dt><dd>{st.network.ip ?? st.local_ip ?? '–'}</dd>
    <dt>Nom sur le réseau</dt><dd>heim.local</dd>
  </dl>
  {#if st.network.last_error}<p class="error">{st.network.last_error}</p>{/if}

  <h2>Changer de Wi-Fi</h2>
  <form onsubmit={join}>
    <label class="field">
      <span>Réseau</span>
      <select class="input" bind:value={ssid}>
        <option value="">{busy ? 'Recherche…' : 'Choisir un réseau'}</option>
        {#each networks as n (n.ssid)}<option value={n.ssid}>{n.ssid} ({n.signal} %)</option>{/each}
      </select>
    </label>
    <label class="field">
      <span>Mot de passe</span>
      <input class="input" type="password" bind:value={password} autocomplete="off" />
    </label>
    <div class="row">
      <button class="btn" type="submit" disabled={!ssid}>Rejoindre</button>
      <button class="btn quiet" type="button" disabled={busy} onclick={scan}>Chercher à nouveau</button>
    </div>
  </form>
  <p class="muted small">Si la maison perd son Wi-Fi plus de 15 minutes, Heim ouvre son réseau « {st.network.hotspot_ssid} » : connectez-vous-y pour lui en donner un autre. Il le referme dès que le Wi-Fi connu revient.</p>
{/if}

<style>
  dl {
    display: grid;
    grid-template-columns: minmax(140px, auto) 1fr;
    margin: 0 0 16px;
  }

  dt,
  dd {
    margin: 0;
    padding: 12px 0;
    border-bottom: 1px solid var(--line);
  }

  dt {
    color: var(--muted);
  }

  dd {
    font-weight: 600;
  }

  h2 {
    margin-top: 26px !important;
  }

  .error {
    color: var(--alert);
    font-weight: 600;
  }

  .row {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
  }

  .small {
    font-size: 14px;
    margin-top: 18px;
  }
</style>
