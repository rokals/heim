<script>
  // Présence par le Wi-Fi : chaque personne a son (ou ses) téléphone(s), Heim regarde s'ils
  // sont connectés au réseau de la maison.
  import { onMount } from 'svelte';
  import Icon from '../Icon.svelte';
  import { api, home, toast } from '../../lib/store.svelte.js';
  import { ago } from '../../lib/house.js';

  let data = $state(null);
  let cfg = $state(null);
  let users = $state([]);
  let found = $state(null);
  let scanning = $state(false);
  let dirty = $state(false);
  let manual = $state({ mac: '', user_id: '' });
  let pick = $state({});

  async function load() {
    const d = await api('/presence').catch(() => null);
    users = await api('/users').catch(() => []);
    data = d;
    if (d) {
      cfg = structuredClone(d.config);
      dirty = false;
    }
  }

  onMount(load);

  const nameOf = (id) => users.find((u) => u.id === id)?.name ?? id;
  const touch = () => (dirty = true);

  async function save() {
    try {
      await api('/presence/config', { method: 'PUT', body: $state.snapshot(cfg) });
      toast('Présence enregistrée');
      await load();
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  async function scan() {
    scanning = true;
    try {
      found = await api('/presence/scan', { method: 'POST' });
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      scanning = false;
    }
  }

  function addPhone(mac, user_id, label = null) {
    if (!user_id) return toast('Choisissez à qui est ce téléphone', 'alert');
    if (cfg.phones.some((p) => p.mac === mac)) return toast('Ce téléphone est déjà suivi', 'alert');
    cfg.phones.push({ mac, user_id, label });
    cfg.enabled = true;
    touch();
  }

  const fmtAgo = (t) => ago(t, home.now);
</script>

<p class="muted">Heim regarde si les téléphones sont connectés au Wi-Fi de la maison. Pas de GPS, rien à installer. Chaque personne peut servir dans les automatismes (« quand Alex rentre… »).</p>

{#if cfg}
  <label class="check big">
    <input type="checkbox" bind:checked={cfg.enabled} onchange={touch} />
    <strong>Suivre la présence</strong>
  </label>

  {#if data.people.length}
    <ul class="people">
      {#each data.people as p (p.user_id)}
        <li class:home={p.home}>
          <span class="avatar" aria-hidden="true">{p.name.slice(0, 1).toUpperCase()}</span>
          <div class="txt">
            <strong>{p.name}</strong>
            <span>{p.home === true ? 'À la maison' : p.home === false ? 'Absent' : 'En attente'} · {p.last_seen ? `vu ${fmtAgo(p.last_seen)}` : 'pas encore vu'}</span>
          </div>
        </li>
      {/each}
    </ul>
  {/if}

  <h2>Téléphones suivis</h2>
  {#if !cfg.phones.length}
    <p class="muted">Aucun pour l'instant.</p>
  {:else}
    <ul class="rows">
      {#each cfg.phones as p, i (p.mac)}
        <li>
          <Icon name="link" size={18} />
          <div class="txt"><strong>{nameOf(p.user_id)}</strong><span class="num">{p.mac}</span></div>
          <button class="btn danger" onclick={() => (cfg.phones.splice(i, 1), touch())}>Retirer</button>
        </li>
      {/each}
    </ul>
  {/if}

  <h2>Ajouter un téléphone</h2>
  <div class="tip">
    <strong>Sur iPhone</strong> : Réglages, Wi-Fi, touchez <span class="i">i</span> à côté du réseau de la maison.
    Mettez <em>Adresse Wi-Fi privée</em> sur <strong>Fixe</strong> (ou désactivée), puis notez l'<em>Adresse Wi-Fi</em> affichée juste en dessous : c'est elle qu'il faut choisir ici.
  </div>

  <button class="btn quiet" disabled={scanning} onclick={scan}>{scanning ? 'Recherche…' : 'Chercher sur le réseau'}</button>

  {#if found}
    {#if !found.length}
      <p class="muted">Rien trouvé. Heim tourne-t-il bien sur le réseau de la maison (et pas sur Tailscale seulement) ?</p>
    {:else}
      <ul class="rows found">
        {#each found as n (n.mac)}
          <li>
            <div class="txt">
              <strong class="num">{n.mac}</strong>
              <span class="num">{n.ip}{n.private ? ' · adresse privée (téléphone ?)' : ''}{n.phone_of ? ` · ${nameOf(n.phone_of)}` : ''}</span>
            </div>
            {#if !n.phone_of}
              <select class="input sel" bind:value={pick[n.mac]} aria-label="À qui est {n.mac}">
                <option value="">À qui ?</option>
                {#each users as u (u.id)}<option value={u.id}>{u.name}</option>{/each}
              </select>
              <button class="btn quiet small" disabled={!pick[n.mac]} onclick={() => addPhone(n.mac, pick[n.mac])}>Ajouter</button>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  {/if}

  <form
    class="manual"
    onsubmit={(e) => {
      e.preventDefault();
      const mac = manual.mac.trim().toLowerCase().replace(/-/g, ':');
      if (!/^([0-9a-f]{2}:){5}[0-9a-f]{2}$/.test(mac)) return toast("Adresse invalide (exemple : aa:bb:cc:12:34:56)", 'alert');
      addPhone(mac, manual.user_id);
      manual = { mac: '', user_id: '' };
    }}
  >
    <input class="input" placeholder="Ou tapez l'adresse : aa:bb:cc:12:34:56" bind:value={manual.mac} autocomplete="off" />
    <select class="input" bind:value={manual.user_id} aria-label="À qui">
      <option value="">À qui ?</option>
      {#each users as u (u.id)}<option value={u.id}>{u.name}</option>{/each}
    </select>
    <button class="btn quiet" type="submit" disabled={!manual.mac || !manual.user_id}>Ajouter</button>
  </form>
  {#if !users.length}
    <p class="muted">Créez d'abord les comptes de la maison dans <a href="#reglages/comptes">Comptes</a>.</p>
  {/if}

  <h2>Réglages</h2>
  <label class="field">
    <span>Considérer parti après</span>
    <span class="unit"><input class="input num" type="number" min="1" max="120" bind:value={cfg.away_after_min} oninput={touch} /> minutes sans nouvelles</span>
  </label>
  <p class="muted small">Un iPhone en veille répond parfois mal : 10 minutes évitent les faux départs.</p>
  <label class="field">
    <span>Quand tout le monde est parti</span>
    <select class="input" bind:value={cfg.everyone_left} onchange={touch}>
      <option value="suggest">Proposer d'activer l'alarme (notification)</option>
      <option value="arm">Activer l'alarme Absent toute seule</option>
      <option value="nothing">Ne rien faire</option>
    </select>
  </label>
  <p class="muted small">L'alarme ne se désactive jamais toute seule quand quelqu'un rentre : un téléphone volé ne doit pas ouvrir la maison. Heim prévient simplement que l'alarme est toujours activée.</p>

  {#if dirty}
    <div class="savebar">
      <button class="btn" onclick={save}>Enregistrer</button>
      <button class="btn quiet" onclick={load}>Annuler</button>
    </div>
  {/if}
{/if}

<style>
  h2 {
    margin-top: 28px !important;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .check input {
    width: 22px;
    height: 22px;
    accent-color: var(--ink);
  }

  .big {
    margin: 14px 0;
    font-size: 18px;
  }

  .people {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 10px;
  }

  .people li {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 12px 14px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .avatar {
    display: grid;
    place-items: center;
    flex: none;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--muted);
    font-weight: 600;
  }

  .people li.home .avatar {
    background: var(--moss);
    color: #fff;
  }

  .txt {
    flex: 1;
    display: grid;
    min-width: 0;
  }

  .txt span {
    font-size: 14px;
    color: var(--muted);
    overflow-wrap: anywhere;
  }

  .rows {
    list-style: none;
    margin: 10px 0;
    padding: 0;
  }

  .rows li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 12px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
  }

  .sel {
    flex: 0 1 140px;
    min-height: 40px;
    padding: 6px 10px;
  }

  .small {
    min-height: 40px;
    padding: 0 14px;
  }

  p.small {
    font-size: 14px;
    margin-top: -8px;
  }

  .tip {
    margin: 0 0 14px;
    padding: 12px 14px;
    border-radius: 14px;
    background: var(--surface);
    font-size: 15px;
  }

  .i {
    display: inline-grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border: 1.5px solid currentColor;
    border-radius: 50%;
    font-size: 12px;
    font-weight: 600;
    font-style: italic;
  }

  .manual {
    display: grid;
    grid-template-columns: 1fr;
    gap: 10px;
    margin-top: 16px;
    max-width: 460px;
  }

  .unit {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--muted);
  }

  .unit .input {
    width: 90px;
  }

  a {
    color: inherit;
    font-weight: 600;
  }

  .savebar {
    position: sticky;
    bottom: calc(80px + env(safe-area-inset-bottom));
    display: flex;
    gap: 10px;
    padding: 10px 0;
    background: var(--bg);
  }
</style>
