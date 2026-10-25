<script>
  // Premier démarrage : bienvenue, compte, Wi-Fi. Ouvert tout seul sur le téléphone connecté
  // au réseau « Heim-XXXX » (portail captif), ou au premier passage sur http://heim.local.
  import Icon from './Icon.svelte';
  import { api, connect, home, loadMe, loadSetup, saveLayout, toast } from '../lib/store.svelte.js';
  import { ROOM_PRESETS } from '../lib/rooms.js';

  // Compte déjà créé (retour sur le réseau Heim) : directement le Wi-Fi.
  let step = $state(home.setup?.accounts ? 'wifi' : 'welcome');
  let account = $state({ name: '', password: '', confirm: '' });
  let networks = $state(home.setup?.network?.networks ?? []);
  let ssid = $state('');
  let other = $state(false);
  let password = $state('');
  let showPassword = $state(false);
  let busy = $state(false);
  let switched = $state(null);

  const net = $derived(home.setup?.network);
  // Le Wi-Fi n'est à régler que sur le Raspberry Pi, et seulement s'il n'est pas déjà relié.
  const needsWifi = $derived(home.setup?.netsetup && net && !['ethernet', 'wifi'].includes(net.mode));
  const steps = $derived(
    home.setup?.accounts ? ['wifi', 'done'] : needsWifi ? ['welcome', 'account', 'house', 'wifi', 'done'] : ['welcome', 'account', 'house', 'done'],
  );

  // La maison : un nom et des pièces, cochées d'un doigt.
  let houseName = $state('');
  let picked = $state(['Salon', 'Cuisine', 'Chambre', 'Salle de bain', 'Entrée']);
  let custom = $state('');

  function togglePick(n) {
    picked = picked.includes(n) ? picked.filter((x) => x !== n) : [...picked, n];
  }

  async function saveHouse(skip = false) {
    if (!skip) {
      busy = true;
      try {
        const rooms = picked.map((name) => ({ name, icon: ROOM_PRESETS.find((p) => p.name === name)?.icon }));
        await saveLayout({ name: houseName.trim(), floors: [], rooms });
      } catch (e) {
        toast(e.message, 'alert');
        busy = false;
        return;
      }
      busy = false;
    }
    if (needsWifi) step = 'wifi';
    else await finish();
  }

  $effect(() => {
    if (step === 'wifi') scan();
  });
  const index = $derived(steps.indexOf(step));

  async function createAccount(e) {
    e.preventDefault();
    if (account.password !== account.confirm) return toast('Les deux mots de passe ne sont pas identiques', 'alert');
    busy = true;
    try {
      await api('/users', { method: 'POST', body: { name: account.name.trim(), password: account.password, role: 'admin' } });
      await loadMe();
      step = 'house';
    } catch (err) {
      toast(err.message, 'alert');
    } finally {
      busy = false;
    }
  }

  async function scan() {
    try {
      const list = await api('/setup/scan', { method: 'POST' });
      if (list.length) networks = list;
    } catch {
      /* on garde la liste vue au démarrage */
    }
  }

  async function joinWifi(e) {
    e.preventDefault();
    busy = true;
    try {
      const r = await api('/setup/wifi', { method: 'POST', body: { ssid, password } });
      switched = r;
      step = 'done';
    } catch (err) {
      toast(err.message, 'alert');
    } finally {
      busy = false;
    }
  }

  async function finish() {
    await api('/setup/done', { method: 'POST' }).catch(() => {});
    step = 'done';
  }

  async function enter() {
    home.setup = null;
    await loadSetup();
    connect();
  }

  function bars(signal) {
    return signal > 70 ? 3 : signal > 40 ? 2 : 1;
  }
</script>

<section class="wizard">
  <ol class="progress" aria-label="Étapes">
    {#each steps as s, i (s)}<li class:on={i <= index}></li>{/each}
  </ol>

  {#if step === 'welcome'}
    <svg viewBox="0 0 32 32" width="64" height="64" aria-hidden="true" class="logo">
      <path d="M6 15 16 7l10 8v11H6z" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linejoin="round" />
      <path d="M13 26v-6h6v6" fill="none" stroke="var(--ember)" stroke-width="2.4" stroke-linejoin="round" />
    </svg>
    <h1>Bienvenue dans Heim</h1>
    <p>Trois minutes pour préparer la maison : votre compte{needsWifi ? ', puis le Wi-Fi' : ''}. Les appareils viendront ensuite.</p>
    <button class="btn wide" onclick={() => (step = 'account')}>Commencer</button>
  {:else if step === 'account'}
    <h1>Votre compte</h1>
    <p>Vous serez administrateur : vous pourrez ensuite créer un compte pour chaque personne de la maison.</p>
    {#if !needsWifi}
      <button class="link skip" type="button" onclick={finish}>Plus tard : laisser Heim ouvert à tout le réseau</button>
    {/if}
    <form onsubmit={createAccount}>
      <label class="field"><span>Prénom</span><input class="input" bind:value={account.name} autocomplete="username" /></label>
      <label class="field"><span>Mot de passe (8 caractères ou plus)</span><input class="input" type="password" bind:value={account.password} autocomplete="new-password" /></label>
      <label class="field"><span>Le même, encore</span><input class="input" type="password" bind:value={account.confirm} autocomplete="new-password" /></label>
      <button class="btn wide" type="submit" disabled={busy || !account.name.trim() || account.password.length < 8}>Continuer</button>
    </form>
  {:else if step === 'house'}
    <h1>Votre maison</h1>
    <p>Cochez vos pièces : elles serviront à ranger les appareils. Tout se change ensuite dans Réglages, Maison.</p>
    <label class="field"><span>Nom de la maison (facultatif)</span><input class="input" bind:value={houseName} placeholder="Chez nous" /></label>
    <div class="picks">
      {#each [...ROOM_PRESETS.map((p) => p.name), ...picked.filter((n) => !ROOM_PRESETS.some((p) => p.name === n))] as n (n)}
        <button type="button" class="pick" class:on={picked.includes(n)} aria-pressed={picked.includes(n)} onclick={() => togglePick(n)}>{n}</button>
      {/each}
    </div>
    <form
      class="addroom"
      onsubmit={(e) => {
        e.preventDefault();
        if (custom.trim() && !picked.includes(custom.trim())) picked = [...picked, custom.trim()];
        custom = '';
      }}
    >
      <input class="input" bind:value={custom} placeholder="Autre pièce" />
      <button class="btn quiet" type="submit" disabled={!custom.trim()}>Ajouter</button>
    </form>
    <button class="btn wide" disabled={busy} onclick={() => saveHouse()}>Continuer ({picked.length} pièce{picked.length > 1 ? 's' : ''})</button>
    <button class="link" type="button" onclick={() => saveHouse(true)}>Passer</button>
  {:else if step === 'wifi'}
    <h1>Le Wi-Fi de la maison</h1>
    <p>Heim va quitter son propre réseau « {net?.hotspot_ssid} » et rejoindre le vôtre.</p>
    {#if net?.last_error}<p class="error" role="alert">{net.last_error}</p>{/if}
    <form onsubmit={joinWifi}>
      {#if !other && networks.length}
        <ul class="nets" role="radiogroup" aria-label="Réseaux Wi-Fi">
          {#each networks as n (n.ssid)}
            <li>
              <label class:chosen={ssid === n.ssid}>
                <input type="radio" name="ssid" value={n.ssid} bind:group={ssid} />
                <span class="bars b{bars(n.signal)}" aria-label="signal {n.signal} %"><i></i><i></i><i></i></span>
                <span class="name">{n.ssid}</span>
                {#if n.secure}<span class="lock" aria-label="protégé">🔒</span>{/if}
              </label>
            </li>
          {/each}
        </ul>
        <button type="button" class="link" onclick={() => ((other = true), (ssid = ''))}>Mon réseau n'est pas dans la liste</button>
      {:else}
        <label class="field"><span>Nom du réseau</span><input class="input" bind:value={ssid} autocomplete="off" autocapitalize="off" /></label>
        {#if networks.length}<button type="button" class="link" onclick={() => (other = false)}>Voir la liste</button>{/if}
      {/if}
      {#if ssid && (other || networks.find((n) => n.ssid === ssid)?.secure !== false)}
        <label class="field pw">
          <span>Mot de passe du Wi-Fi</span>
          <input class="input" type={showPassword ? 'text' : 'password'} bind:value={password} autocomplete="off" autocapitalize="off" />
          <button type="button" class="link" onclick={() => (showPassword = !showPassword)}>{showPassword ? 'Masquer' : 'Afficher'}</button>
        </label>
      {/if}
      <button class="btn wide" type="submit" disabled={busy || !ssid}>Rejoindre ce Wi-Fi</button>
    </form>
  {:else}
    <span class="ok"><Icon name="check" size={34} stroke={2.4} /></span>
    {#if switched}
      <h1>Heim rejoint « {switched.ssid} »</h1>
      <ol class="next">
        <li>Reconnectez ce téléphone à votre Wi-Fi « {switched.ssid} ».</li>
        <li>Ouvrez <strong>{switched.next}</strong> (ou l'adresse du Raspberry Pi dans votre box).</li>
        <li>Connectez-vous avec le compte que vous venez de créer.</li>
      </ol>
      <p class="muted">Si le réseau « {switched.hotspot_ssid} » réapparaît d'ici une minute, c'est que le mot de passe était faux : reconnectez-vous-y, l'assistant reprendra ici.</p>
    {:else}
      <h1>C'est prêt</h1>
      <p>Prochaine étape : relier vos appareils (Zigbee2MQTT, Tuya, Matter) dans Réglages.</p>
      <button class="btn wide" onclick={enter}>Entrer dans Heim</button>
    {/if}
  {/if}
</section>

<style>
  .wizard {
    max-width: 440px;
    margin: 4vh auto 0;
  }

  .progress {
    display: flex;
    gap: 6px;
    list-style: none;
    padding: 0;
    margin: 0 0 28px;
  }

  .progress li {
    flex: 1;
    height: 4px;
    border-radius: 2px;
    background: var(--line);
  }

  .progress li.on {
    background: var(--ink);
  }

  .logo {
    display: block;
    margin: 8px 0 12px;
  }

  h1 {
    font-size: 32px;
    line-height: 1.15;
    margin: 0 0 12px;
  }

  p {
    margin: 0 0 20px;
  }

  .muted {
    color: var(--muted);
    font-size: 15px;
  }

  .wide {
    width: 100%;
    justify-content: center;
    min-height: 52px;
    font-size: 17px;
  }

  .error {
    padding: 10px 14px;
    border-radius: 12px;
    background: color-mix(in srgb, var(--alert) 14%, var(--surface));
    color: var(--alert);
    font-weight: 600;
  }

  .nets {
    list-style: none;
    padding: 0;
    margin: 0 0 8px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    overflow: hidden;
  }

  .nets li + li {
    border-top: 1px solid var(--line);
  }

  .nets label {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 54px;
    padding: 0 14px;
    cursor: pointer;
  }

  .nets label.chosen {
    background: var(--surface-2);
    font-weight: 600;
  }

  .nets input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }

  .name {
    flex: 1;
    overflow-wrap: anywhere;
  }

  .bars {
    display: inline-flex;
    align-items: flex-end;
    gap: 2px;
    height: 16px;
  }

  .bars i {
    width: 4px;
    border-radius: 1px;
    background: var(--line);
  }

  .bars i:nth-child(1) {
    height: 6px;
  }

  .bars i:nth-child(2) {
    height: 11px;
  }

  .bars i:nth-child(3) {
    height: 16px;
  }

  .b1 i:nth-child(1),
  .b2 i:nth-child(-n + 2),
  .b3 i {
    background: var(--ink);
  }

  .link {
    color: var(--muted);
    font-weight: 600;
    min-height: 40px;
    margin-bottom: 12px;
  }

  .pw {
    margin-top: 12px;
  }

  .picks {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 0 0 14px;
  }

  .pick {
    padding: 9px 14px;
    border-radius: var(--radius-chip);
    background: var(--surface);
    box-shadow: inset 0 0 0 1px var(--line);
    font-weight: 600;
  }

  .pick.on {
    background: var(--ink);
    color: var(--bg);
    box-shadow: none;
  }

  .addroom {
    display: flex;
    gap: 10px;
    margin-bottom: 18px;
  }

  .skip {
    margin: -8px 0 12px;
    font-size: 14px;
  }

  .pw .link {
    justify-self: start;
    margin: 0;
  }

  .ok {
    display: grid;
    place-items: center;
    width: 64px;
    height: 64px;
    border-radius: 50%;
    background: var(--moss);
    color: #fff;
    margin-bottom: 16px;
  }

  .next {
    padding-left: 22px;
    display: grid;
    gap: 8px;
    margin: 0 0 18px;
  }
</style>
