<script>
  // Premier démarrage, façon installateur : Langue → Compte → Réseau → Apparence → Maison →
  // Résumé. Langue, compte et réseau sont obligatoires (on ne démarre pas sans compte) ;
  // l'apparence et la maison se passent. L'étape est retenue par le Raspberry Pi : après le
  // passage sur le Wi-Fi de la maison, on se reconnecte sur heim.local et on reprend ici.
  import { onDestroy, onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import LookEditor from './settings/LookEditor.svelte';
  import { tr, LANGS, currentLang, setLang } from '../lib/i18n.js';
  import { api, connect, home, loadLook, loadMe, loadSetup, saveLayout, toast } from '../lib/store.svelte.js';
  import { ROOM_PRESETS } from '../lib/rooms.js';
  import { cleanLook } from '../lib/theme.js';
  import ZigbeeLocal from './settings/ZigbeeLocal.svelte';

  const STEPS = [
    { id: 'language', label: tr('Langue'), icon: 'globe' },
    { id: 'account', label: tr('Compte'), icon: 'user' },
    { id: 'network', label: tr('Réseau'), icon: 'wifi' },
    { id: 'look', label: tr('Apparence'), icon: 'sun', optional: true },
    { id: 'house', label: tr('Maison'), icon: 'house', optional: true },
    { id: 'connections', label: tr('Connexions'), icon: 'link', optional: true },
    { id: 'summary', label: tr('Résumé'), icon: 'check' },
  ];
  const KEY = 'heim_setup_step';

  let status = $state(home.setup ?? null);
  const hasAccount = $derived(Boolean(status?.accounts));
  const net = $derived(status?.network ?? null);
  const managed = $derived(Boolean(status?.netsetup) && net && net.mode !== 'unmanaged');
  const onHotspot = $derived(typeof location !== 'undefined' && location.hostname === '10.42.0.1');

  function startStep() {
    if (home.setup?.accounts) return home.setup.resume && home.setup.resume !== 'account' ? home.setup.resume : 'network';
    let s = null;
    try {
      s = sessionStorage.getItem(KEY);
    } catch {
      /* rien */
    }
    return s === 'account' ? 'account' : 'language';
  }

  let step = $state(startStep());
  const index = $derived(STEPS.findIndex((s) => s.id === step));
  let skipped = $state([]);
  let busy = $state(false);

  async function refresh() {
    try {
      status = await fetch('/api/setup', { headers: { 'X-Heim-Lang': currentLang() } }).then((r) => r.json());
    } catch {
      /* le Pi change peut-être de réseau */
    }
  }

  async function remember(id) {
    try {
      sessionStorage.setItem(KEY, id);
    } catch {
      /* rien */
    }
    if (hasAccount) await api('/setup/step', { method: 'POST', body: { step: id } }).catch(() => {});
  }

  async function go(id) {
    step = id;
    await remember(id);
    scrollTo({ top: 0 });
  }

  const next = () => go(STEPS[index + 1].id);
  const prev = () => go(STEPS[Math.max(0, index - 1)].id);

  // ---- 1. Langue ----
  let lang = $state(currentLang());

  async function saveLanguage() {
    busy = true;
    // La langue de la maison : celle de tous, tant que chacun n'a pas choisi la sienne.
    const house = await api('/appearance').then((r) => r.house ?? {}).catch(() => ({}));
    await api('/appearance', { method: 'PUT', body: cleanLook({ ...house, lang }) }).catch(() => {});
    busy = false;
    next();
  }

  function pickLang(id) {
    lang = id;
    setLang(id); // recharge la page dans la nouvelle langue si besoin
  }

  // ---- 2. Compte ----
  let account = $state({ name: '', password: '', confirm: '' });
  let showPw = $state(false);
  const pwOk = $derived(account.password.length >= 8);
  const sameOk = $derived(account.password === account.confirm);
  const accountOk = $derived(account.name.trim().length > 0 && pwOk && sameOk);
  const strength = $derived.by(() => {
    const p = account.password;
    let n = 0;
    if (p.length >= 8) n++;
    if (p.length >= 12) n++;
    if (/[A-Z]/.test(p) && /[a-z]/.test(p)) n++;
    if (/\d/.test(p) && /[^\w]/.test(p)) n++;
    return n;
  });

  async function createAccount() {
    busy = true;
    try {
      await api('/users', { method: 'POST', body: { name: account.name.trim(), password: account.password, role: 'admin' } });
      await loadMe();
      await refresh();
      await api('/setup/step', { method: 'POST', body: { step: 'network' } }).catch(() => {});
      account = { name: '', password: '', confirm: '' };
      step = 'network';
      await remember('network');
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  // ---- 3. Réseau ----
  let via = $state(null); // 'cable' | 'wifi'
  let networks = $state([]);
  let ssid = $state('');
  let hidden = $state(false);
  let password = $state('');
  let showWifiPw = $state(false);
  let advanced = $state(false);
  let fixed = $state({ address: '', gateway: '', dns: '' });
  let hostname = $state('');
  let switched = $state(null);
  let poll = null;

  const connected = $derived(net && ['ethernet', 'wifi'].includes(net.mode) && !onHotspot);
  const networkOk = $derived(!managed || connected);

  $effect(() => {
    if (step !== 'network' || !managed) return;
    if (!hostname) hostname = status?.hostname || 'heim';
    // Le câble se branche pendant qu'on regarde : on le voit arriver.
    poll = setInterval(refresh, 3000);
    return () => clearInterval(poll);
  });

  $effect(() => {
    if (step === 'network' && via === 'wifi' && !networks.length) scan();
  });

  async function scan() {
    try {
      const list = await api('/setup/scan', { method: 'POST' });
      networks = list.length ? list : (net?.networks ?? []);
    } catch {
      networks = net?.networks ?? [];
    }
  }

  async function applyHostname() {
    const name = hostname.trim().toLowerCase();
    if (!name || name === (status?.hostname || 'heim')) return true;
    try {
      await api('/setup/hostname', { method: 'POST', body: { name } });
      return true;
    } catch (e) {
      toast(e.message, 'alert');
      return false;
    }
  }

  async function joinWifi() {
    busy = true;
    if (!(await applyHostname())) return (busy = false);
    await remember('look');
    try {
      const body = { ssid: ssid.trim(), password, hidden };
      if (advanced && fixed.address.trim()) body.static = $state.snapshot(fixed);
      switched = await api('/setup/wifi', { method: 'POST', body });
    } catch (e) {
      toast(e.message, 'alert');
      await remember('network');
    } finally {
      busy = false;
    }
  }

  async function useCable() {
    busy = true;
    if (!(await applyHostname())) return (busy = false);
    await remember('look');
    try {
      switched = { ...(await api('/setup/ethernet', { method: 'POST' })), cable: true };
    } catch (e) {
      toast(e.message, 'alert');
      await remember('network');
    } finally {
      busy = false;
    }
  }

  function bars(signal) {
    return signal > 70 ? 3 : signal > 40 ? 2 : 1;
  }

  // ---- 4. Apparence ----
  let look = $state({});
  onMount(async () => {
    const a = await api('/appearance').catch(() => null);
    if (a?.house) look = { ...a.house };
  });

  async function saveLook() {
    busy = true;
    const house = await api('/appearance').then((r) => r.house ?? {}).catch(() => ({}));
    await api('/appearance', { method: 'PUT', body: cleanLook({ ...house, ...$state.snapshot(look), lang: house.lang }) }).catch((e) =>
      toast(e.message, 'alert'),
    );
    await loadLook();
    busy = false;
    next();
  }

  // ---- 5. Maison ----
  let houseName = $state('');
  // Salon, cuisine, chambre, salle de bain, entrée (dans la langue choisie).
  let picked = $state([0, 1, 2, 5, 7].map((i) => ROOM_PRESETS[i]?.name).filter(Boolean));
  let custom = $state('');
  let lat = $state('');
  let lon = $state('');
  const browserTz = Intl.DateTimeFormat().resolvedOptions().timeZone;

  function togglePick(n) {
    picked = picked.includes(n) ? picked.filter((x) => x !== n) : [...picked, n];
  }

  function locate() {
    if (!navigator.geolocation) return toast(tr("Ce navigateur ne donne pas sa position. Saisissez-la à la main."), 'alert');
    navigator.geolocation.getCurrentPosition(
      (p) => {
        lat = String(Math.round(p.coords.latitude * 100) / 100);
        lon = String(Math.round(p.coords.longitude * 100) / 100);
      },
      () => toast(tr("Position refusée ou indisponible (souvent le cas hors https). Saisissez-la à la main."), 'alert'),
    );
  }

  async function saveHouse() {
    busy = true;
    try {
      const rooms = picked.map((name) => ({ name, icon: ROOM_PRESETS.find((p) => p.name === name)?.icon }));
      await saveLayout({ name: houseName.trim(), floors: [], rooms });
      if (lat !== '' && lon !== '') {
        await api('/settings/location', { method: 'PUT', body: { latitude: Number(lat), longitude: Number(lon) } });
      }
      if (managed && browserTz && browserTz !== status?.timezone) {
        await api('/setup/timezone', { method: 'POST', body: { timezone: browserTz } }).catch(() => {});
      }
      next();
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  // ---- 6. Connexions : le broker MQTT (rien à régler) et Zigbee sur la box ----
  let zigbeeOn = $state(false);
  let zigbeeInstalled = $state(null);
  let broker = $state(null);

  $effect(() => {
    if (step === 'connections' && zigbeeInstalled === null) {
      api('/zigbee/local')
        .then((z) => {
          zigbeeInstalled = z.installed;
          zigbeeOn = z.enabled;
        })
        .catch(() => (zigbeeInstalled = false));
      api('/broker')
        .then((b) => (broker = b))
        .catch(() => {});
    }
  });

  async function brokerOpen(open) {
    try {
      broker = await api('/broker', { method: 'PUT', body: { enabled: true, open, port: broker?.settings?.port } });
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  // ---- 7. Résumé ----
  let autoUpdate = $state(true);

  async function finish() {
    busy = true;
    try {
      await api('/update/settings', { method: 'PUT', body: { auto_check: autoUpdate } }).catch(() => {});
      await api('/setup/done', { method: 'POST' });
      try {
        sessionStorage.removeItem(KEY);
      } catch {
        /* rien */
      }
      home.setup = null;
      await loadSetup();
      connect();
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  function skip() {
    skipped = [...new Set([...skipped, step])];
    next();
  }

  onDestroy(() => clearInterval(poll));

  const me = $derived(home.me?.user);
  const netLine = $derived.by(() => {
    if (!managed) return tr('Géré par cette machine');
    if (net?.mode === 'ethernet') return tr('Câble réseau{ip}', { ip: net.ethernet_ip ? ` · ${net.ethernet_ip}` : '' });
    if (net?.mode === 'wifi') return tr('Wi-Fi « {ssid} »', { ssid: net.ssid });
    return tr('Pas encore relié');
  });
</script>

<div class="installer">
  <aside class="steps" aria-label={tr('Étapes')}>
    <div class="brand">
      <svg viewBox="0 0 32 32" width="34" height="34" aria-hidden="true">
        <path d="M6 15 16 7l10 8v11H6z" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linejoin="round" />
        <path d="M13 26v-6h6v6" fill="none" stroke="var(--ember)" stroke-width="2.4" stroke-linejoin="round" />
      </svg>
      <span>Heim</span>
      <em>{status?.version ? `v${status.version}` : ''}</em>
    </div>
    <ol>
      {#each STEPS as s, i (s.id)}
        <li class:done={i < index} class:current={i === index}>
          <span class="num">{#if i < index}<Icon name="check" size={14} stroke={2.6} />{:else}{i + 1}{/if}</span>
          <span class="lbl">{s.label}{#if s.optional}<small>{tr('facultatif')}</small>{/if}</span>
        </li>
      {/each}
    </ol>
    <div class="mobile">
      <span>{tr('Étape {n} sur {total}', { n: index + 1, total: STEPS.length })} · <strong>{STEPS[index].label}</strong></span>
      <div class="bar"><i style:width="{((index + 1) / STEPS.length) * 100}%"></i></div>
    </div>
  </aside>

  <section class="page" aria-live="polite">
    {#if step === 'language'}
      <h1>{tr('Bienvenue dans Heim')}</h1>
      <p class="lead">{tr('Votre box domotique, chez vous et sans cloud. Quelques minutes pour la préparer.')}</p>
      <h2>{tr('Choisissez votre langue')}</h2>
      <div class="langs" role="radiogroup" aria-label={tr('Langue')}>
        {#each LANGS as l (l.id)}
          <button class="choice" class:sel={lang === l.id} role="radio" aria-checked={lang === l.id} onclick={() => pickLang(l.id)}>
            <span class="code">{l.id.toUpperCase()}</span>
            <strong>{l.label}</strong>
            {#if lang === l.id}<Icon name="check" size={18} />{/if}
          </button>
        {/each}
      </div>
      <p class="muted small">{tr('Chacun pourra choisir la sienne ensuite, dans Réglages, Apparence.')}</p>
    {:else if step === 'account'}
      <h1>{tr('Votre compte')}</h1>
      <p class="lead">{tr('Vous serez administrateur. Un compte est obligatoire : sans lui, n’importe qui sur le réseau pourrait tout commander.')}</p>
      {#if hasAccount}
        <div class="state ok"><Icon name="check" size={20} /> {tr('Compte créé : {name}', { name: me?.name ?? '' })}</div>
      {:else}
        <form onsubmit={(e) => (e.preventDefault(), accountOk && createAccount())}>
          <label class="field"><span>{tr('Prénom')}</span><input class="input" bind:value={account.name} autocomplete="username" /></label>
          <label class="field">
            <span>{tr('Mot de passe (8 caractères ou plus)')}</span>
            <input class="input" type={showPw ? 'text' : 'password'} bind:value={account.password} autocomplete="new-password" />
          </label>
          {#if account.password}
            <div class="meter" aria-label={tr('Solidité du mot de passe')}>
              {#each [0, 1, 2, 3] as k (k)}<i class:on={k < strength} class="s{strength}"></i>{/each}
              <span>{[tr('Trop court'), tr('Faible'), tr('Correct'), tr('Bon'), tr('Excellent')][strength]}</span>
            </div>
          {/if}
          <label class="field">
            <span>{tr('Le même, encore')}</span>
            <input class="input" type={showPw ? 'text' : 'password'} bind:value={account.confirm} autocomplete="new-password" />
          </label>
          {#if account.confirm && !sameOk}<p class="warn">{tr('Les deux mots de passe ne sont pas identiques')}</p>{/if}
          <label class="check"><input type="checkbox" bind:checked={showPw} /> {tr('Afficher les mots de passe')}</label>
          <button type="submit" hidden aria-hidden="true"></button>
        </form>
      {/if}
    {:else if step === 'network'}
      <h1>{tr('Le réseau')}</h1>
      {#if switched}
        <div class="state ok"><Icon name="check" size={20} /> {switched.cable ? tr('Heim passe sur le câble réseau') : tr('Heim rejoint « {ssid} »', { ssid: switched.ssid })}</div>
        <ol class="howto">
          <li>{tr('Reconnectez ce téléphone au Wi-Fi de la maison.')}</li>
          <li>{tr('Ouvrez')} <strong>{switched.next}</strong> {tr('(ou l’adresse du Raspberry Pi dans votre box).')}</li>
          <li>{tr('Connectez-vous avec le compte que vous venez de créer : l’installation reprend à l’étape suivante.')}</li>
        </ol>
        {#if !switched.cable}
          <p class="muted small">{tr('Si le réseau « {hotspot} » réapparaît d’ici une minute, le mot de passe était faux : reconnectez-vous-y, vous reviendrez ici.', { hotspot: switched.hotspot_ssid })}</p>
        {/if}
      {:else if !managed}
        <p class="lead">{tr('Le réseau de cette machine est géré par son système : rien à régler ici.')}</p>
        <div class="state ok"><Icon name="check" size={20} /> {tr('Connecté')}{status?.local_ip ? ` · ${status.local_ip}` : ''}</div>
      {:else}
        <p class="lead">{tr('Comment relier la box à la maison ?')}</p>
        {#if net?.last_error}<p class="error" role="alert">{net.last_error}</p>{/if}
        {#if connected}
          <div class="state ok">
            <Icon name="check" size={20} />
            {net.mode === 'ethernet' ? tr('Connecté par câble') : tr('Connecté au Wi-Fi « {ssid} »', { ssid: net.ssid })}{net.ethernet_ip || net.ip ? ` · ${net.ethernet_ip || net.ip}` : ''}
          </div>
        {:else}
          <div class="vias">
            <button class="choice big" class:sel={via === 'cable'} onclick={() => (via = 'cable')}>
              <Icon name="link" size={22} />
              <strong>{tr('Câble réseau')}</strong>
              <span>{tr('Le plus fiable : branché sur la box Internet')}</span>
            </button>
            <button class="choice big" class:sel={via === 'wifi'} onclick={() => (via = 'wifi')}>
              <Icon name="wifi" size={22} />
              <strong>{tr('Wi-Fi')}</strong>
              <span>{tr('Sans fil, n’importe où dans la maison')}</span>
            </button>
          </div>

          {#if via === 'cable'}
            {#if net?.ethernet}
              <div class="state ok"><Icon name="check" size={20} /> {tr('Câble détecté')}{net.ethernet_ip ? ` · ${net.ethernet_ip}` : ''}</div>
            {:else}
              <div class="state wait"><span class="spinner" aria-hidden="true"></span> {tr('Branchez le câble entre le Raspberry Pi et votre box Internet…')}</div>
            {/if}
          {:else if via === 'wifi'}
            <div class="field">
              <span>{tr('Réseau')}</span>
              {#if !hidden && networks.length}
                <ul class="nets" role="radiogroup" aria-label={tr('Réseaux Wi-Fi')}>
                  {#each networks as n (n.ssid)}
                    <li>
                      <label class:chosen={ssid === n.ssid}>
                        <input type="radio" name="ssid" value={n.ssid} bind:group={ssid} />
                        <span class="bars b{bars(n.signal)}" aria-label={tr('signal {signal} %', { signal: n.signal })}><i></i><i></i><i></i></span>
                        <span class="name">{n.ssid}</span>
                        {#if n.secure}<Icon name="lock" size={15} />{/if}
                      </label>
                    </li>
                  {/each}
                </ul>
              {:else if !hidden}
                <p class="muted small">{tr('Recherche des réseaux…')}</p>
              {/if}
              <label class="check"><input type="checkbox" bind:checked={hidden} onchange={() => (ssid = '')} /> {tr('Réseau caché (absent de la liste)')}</label>
              {#if hidden}
                <input class="input" bind:value={ssid} placeholder={tr('Nom du réseau')} autocomplete="off" autocapitalize="off" />
              {/if}
            </div>
            {#if ssid && (hidden || networks.find((n) => n.ssid === ssid)?.secure !== false)}
              <label class="field">
                <span>{tr('Mot de passe du Wi-Fi')}</span>
                <input class="input" type={showWifiPw ? 'text' : 'password'} bind:value={password} autocomplete="off" autocapitalize="off" />
              </label>
              <label class="check"><input type="checkbox" bind:checked={showWifiPw} /> {tr('Afficher le mot de passe')}</label>
            {/if}
            <details bind:open={advanced}>
              <summary>{tr('Options avancées : adresse fixe')}</summary>
              <p class="muted small">{tr('Laissez vide pour une adresse donnée par la box (le plus simple).')}</p>
              <div class="three">
                <label class="field"><span>{tr('Adresse IP')}</span><input class="input" bind:value={fixed.address} placeholder="192.168.1.50/24" inputmode="decimal" /></label>
                <label class="field"><span>{tr('Passerelle')}</span><input class="input" bind:value={fixed.gateway} placeholder="192.168.1.1" inputmode="decimal" /></label>
                <label class="field"><span>{tr('DNS')}</span><input class="input" bind:value={fixed.dns} placeholder="1.1.1.1" /></label>
              </div>
            </details>
          {/if}

          {#if via}
            <label class="field">
              <span>{tr('Nom de la box sur le réseau')}</span>
              <div class="host">
                <span>http://</span>
                <input class="input" bind:value={hostname} autocapitalize="off" autocomplete="off" aria-label={tr('Nom de la box')} />
                <span>.local</span>
              </div>
            </label>
          {/if}
        {/if}
      {/if}
    {:else if step === 'look'}
      <h1>{tr('Apparence')}</h1>
      <p class="lead">{tr('Pour toute la maison, par défaut. Chacun pourra choisir la sienne. Vous pouvez aussi passer : l’apparence d’origine est très bien.')}</p>
      <LookEditor {look} lang={false} onchange={(l) => (look = l)} />
    {:else if step === 'house'}
      <h1>{tr('Votre maison')}</h1>
      <p class="lead">{tr('Cochez vos pièces : elles serviront à ranger les appareils. Tout se change ensuite dans Réglages, Maison.')}</p>
      <label class="field"><span>{tr('Nom de la maison (facultatif)')}</span><input class="input" bind:value={houseName} placeholder={tr('Chez nous')} /></label>
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
        <input class="input" bind:value={custom} placeholder={tr('Autre pièce')} />
        <button class="btn quiet" type="submit" disabled={!custom.trim()}>{tr('Ajouter')}</button>
      </form>
      <h2>{tr('Lieu et heure')}</h2>
      <p class="muted small">{tr('Le lieu sert seulement au lever et au coucher du soleil, calculés sans Internet. Deux décimales suffisent.')}</p>
      <div class="two">
        <label class="field"><span>{tr('Latitude')}</span><input class="input num" type="number" step="0.01" bind:value={lat} placeholder="47.21" /></label>
        <label class="field"><span>{tr('Longitude')}</span><input class="input num" type="number" step="0.01" bind:value={lon} placeholder="-1.55" /></label>
      </div>
      <button class="btn quiet" type="button" onclick={locate}><Icon name="sun" size={16} /> {tr('Utiliser ma position')}</button>
      {#if managed && browserTz}
        <p class="muted small tz">{tr('Fuseau horaire : {tz} (celui de ce téléphone).', { tz: browserTz })}</p>
      {/if}
    {:else if step === 'connections'}
      <h1>{tr('Connexions')}</h1>
      <p class="lead">{tr('Ce qui relie Heim à vos appareils. Tuya et Matter se règlent ensuite dans Réglages, Connexions.')}</p>
      <h2>{tr('Broker MQTT')}</h2>
      {#if broker?.running}
        <p class="ok-line"><Icon name="check" size={18} /> {tr('Le broker MQTT de Heim tourne déjà : rien à régler, ni compte ni mot de passe. Zigbee2MQTT s’y branche tout seul.')}</p>
        <label class="check big">
          <input type="checkbox" checked={broker.settings.open} onchange={(e) => brokerOpen(e.currentTarget.checked)} />
          <span><strong>{tr('Ouvrir au réseau de la maison')}</strong><em>{tr('Seulement pour des appareils Tasmota, Shelly ou ESPHome. Leurs comptes se créent dans Réglages, Connexions, MQTT.')}</em></span>
        </label>
      {:else if broker?.settings?.enabled === false}
        <p class="muted">{tr('Heim lit Zigbee par le broker d’une autre machine : le sien reste éteint.')}</p>
      {:else if broker?.error}
        <p class="muted">{broker.error}</p>
      {:else}
        <p class="muted">{tr('Chargement…')}</p>
      {/if}
      <h2>{tr('Zigbee')}</h2>
      {#if zigbeeInstalled === false}
        <p class="muted">{tr('Zigbee2MQTT n’est pas installé sur cette machine. Vous pourrez brancher Heim sur un Zigbee2MQTT existant dans Réglages, Connexions.')}</p>
      {:else if zigbeeInstalled}
        <p class="muted">{tr('Branchez la clé Zigbee sur la box : Zigbee2MQTT tourne ici, avec le broker MQTT de Heim. Rien d’autre à installer.')}</p>
        <ZigbeeLocal compact onstarted={() => (zigbeeOn = true)} />
        <p class="muted small">{tr('Vos appareils sont déjà dans un Zigbee2MQTT (Home Assistant) ? Passez cette étape : Réglages, Connexions, Zigbee permet de le reprendre sans rien réappairer.')}</p>
      {/if}
    {:else}
      <h1>{tr('Tout est prêt')}</h1>
      <p class="lead">{tr('Vérifiez, puis entrez dans Heim. Tout se change ensuite dans Réglages.')}</p>
      <dl class="recap">
        <div><dt>{tr('Langue')}</dt><dd>{LANGS.find((l) => l.id === currentLang())?.label}</dd></div>
        <div><dt>{tr('Compte')}</dt><dd>{me?.name ?? '–'} · {tr('administrateur')}</dd></div>
        <div><dt>{tr('Réseau')}</dt><dd>{netLine}</dd></div>
        <div><dt>{tr('Apparence')}</dt><dd>{skipped.includes('look') || !Object.keys(look).some((k) => k !== 'lang') ? tr('D’origine') : tr('Choisie')}</dd></div>
        <div><dt>{tr('Maison')}</dt><dd>{skipped.includes('house') ? tr('Plus tard') : tr('{n} pièce(s)', { n: picked.length })}</dd></div>
        <div><dt>{tr('Broker MQTT')}</dt><dd>{broker?.running ? (broker.settings.open ? tr('Sur la box, ouvert au réseau') : tr('Sur la box')) : tr('Éteint')}</dd></div>
        <div><dt>{tr('Zigbee')}</dt><dd>{zigbeeOn ? tr('Sur la box') : tr('Plus tard')}</dd></div>
      </dl>
      <label class="check big">
        <input type="checkbox" bind:checked={autoUpdate} />
        <span><strong>{tr('Vérifier les mises à jour')}</strong><em>{tr('Deux fois par jour. Rien ne s’installe sans votre accord.')}</em></span>
      </label>
      <p class="muted small">{zigbeeOn ? tr('Prochaine étape : ajouter vos appareils Zigbee (Réglages, Connexions, Zigbee), et Tuya ou Matter si vous en avez.') : tr('Prochaine étape : relier vos appareils (Zigbee, Tuya, Matter) dans Réglages, Connexions.')}</p>
    {/if}

    {#if !switched}
      <footer class="nav">
        {#if index > 0}
          <button class="btn quiet" onclick={prev} disabled={busy}>{tr('Précédent')}</button>
        {:else}
          <span></span>
        {/if}
        <div class="right">
          {#if STEPS[index].optional}<button class="link" onclick={skip} disabled={busy}>{tr('Passer')}</button>{/if}
          {#if step === 'language'}
            <button class="btn" onclick={saveLanguage} disabled={busy}>{tr('Suivant')}</button>
          {:else if step === 'account'}
            <button class="btn" onclick={() => (hasAccount ? next() : createAccount())} disabled={busy || (!hasAccount && !accountOk)}>{tr('Suivant')}</button>
          {:else if step === 'network'}
            {#if managed && !connected && via === 'wifi'}
              <button class="btn" onclick={joinWifi} disabled={busy || !ssid.trim()}>{tr('Rejoindre ce Wi-Fi')}</button>
            {:else if managed && !connected && via === 'cable'}
              <button class="btn" onclick={useCable} disabled={busy || !net?.ethernet}>{tr('Continuer par câble')}</button>
            {:else if managed && net?.mode === 'ethernet' && onHotspot}
              <button class="btn" onclick={useCable} disabled={busy}>{tr('Continuer par câble')}</button>
            {:else}
              <button class="btn" onclick={next} disabled={busy || !networkOk}>{tr('Suivant')}</button>
            {/if}
          {:else if step === 'look'}
            <button class="btn" onclick={saveLook} disabled={busy}>{tr('Suivant')}</button>
          {:else if step === 'house'}
            <button class="btn" onclick={saveHouse} disabled={busy}>{tr('Suivant')}</button>
          {:else if step === 'connections'}
            <button class="btn" onclick={next} disabled={busy}>{tr('Suivant')}</button>
          {:else}
            <button class="btn" onclick={finish} disabled={busy || !hasAccount}>{tr('Entrer dans Heim')}</button>
          {/if}
        </div>
      </footer>
    {/if}
  </section>
</div>

<style>
  .installer {
    display: grid;
    grid-template-columns: 240px minmax(0, 1fr);
    max-width: 960px;
    min-height: min(640px, calc(100vh - 140px));
    margin: 3vh auto 0;
    border-radius: 24px;
    background: var(--surface);
    box-shadow: 0 1px 0 rgba(0, 0, 0, 0.04), 0 18px 50px -20px rgba(0, 0, 0, 0.25);
    overflow: hidden;
  }

  .steps {
    padding: 26px 20px;
    background: var(--surface-2);
    border-right: 1px solid var(--line);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 30px;
    font-weight: 700;
    font-size: 20px;
  }

  .brand em {
    font-style: normal;
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
    margin-left: auto;
  }

  .steps ol {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 4px;
  }

  .steps li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 9px 10px;
    border-radius: 12px;
    color: var(--muted);
    font-weight: 600;
  }

  .steps li.current {
    background: var(--surface);
    color: var(--ink);
    box-shadow: var(--shadow);
  }

  .steps li.done {
    color: var(--ink);
  }

  .num {
    flex: none;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    border: 1.5px solid var(--line);
    font-size: 13px;
  }

  .current .num {
    border-color: var(--ink);
    background: var(--ink);
    color: var(--bg);
  }

  .done .num {
    border-color: var(--moss);
    background: var(--moss);
    color: #fff;
  }

  .lbl {
    display: grid;
  }

  .lbl small {
    font-size: 11px;
    font-weight: 500;
    color: var(--muted);
  }

  .mobile {
    display: none;
  }

  .page {
    display: flex;
    flex-direction: column;
    padding: 36px 40px 28px;
    min-width: 0;
  }

  h1 {
    font-size: 32px;
    line-height: 1.15;
    margin: 0 0 10px;
    letter-spacing: -0.01em;
  }

  h2 {
    font-size: 18px;
    margin: 22px 0 10px;
  }

  .lead {
    margin: 0 0 18px;
    color: var(--muted);
    font-size: 17px;
  }

  .small {
    font-size: 14px;
  }

  .muted {
    color: var(--muted);
  }

  .langs,
  .vias {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
    gap: 10px;
    margin-bottom: 14px;
  }

  .choice {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 16px;
    border-radius: 16px;
    border: 1.5px solid var(--line);
    background: var(--bg);
    text-align: left;
    font-size: 16px;
  }

  .choice.big {
    display: grid;
    gap: 4px;
    align-content: start;
  }

  .choice.big span {
    font-size: 13px;
    color: var(--muted);
  }

  .choice.sel {
    border-color: var(--ink);
    box-shadow: inset 0 0 0 1px var(--ink);
  }

  .choice :global(svg:last-child) {
    margin-left: auto;
  }

  .code {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 10px;
    background: var(--surface-2);
    font-size: 12px;
    font-weight: 800;
    letter-spacing: 0.04em;
  }

  .state {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    margin: 6px 0 14px;
    border-radius: 14px;
    font-weight: 600;
  }

  .state.ok {
    background: color-mix(in srgb, var(--moss) 16%, var(--surface));
    color: color-mix(in srgb, var(--moss) 70%, var(--ink));
  }

  .state.wait {
    background: var(--surface-2);
  }

  .spinner {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    border: 2.5px solid var(--line);
    border-top-color: var(--ink);
    animation: spin 0.9s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(1turn);
    }
  }

  .error {
    padding: 10px 14px;
    border-radius: 12px;
    background: color-mix(in srgb, var(--alert) 14%, var(--surface));
    color: var(--alert);
    font-weight: 600;
  }

  .warn {
    margin: -4px 0 8px;
    color: var(--alert);
    font-size: 14px;
    font-weight: 600;
  }

  .meter {
    display: flex;
    align-items: center;
    gap: 4px;
    margin: -2px 0 10px;
    font-size: 13px;
    color: var(--muted);
  }

  .meter i {
    width: 34px;
    height: 5px;
    border-radius: 3px;
    background: var(--line);
  }

  .meter i.on.s1 {
    background: var(--alert);
  }

  .meter i.on.s2 {
    background: #f76b15;
  }

  .meter i.on.s3,
  .meter i.on.s4 {
    background: var(--moss);
  }

  .meter span {
    margin-left: 6px;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 8px 0;
    font-size: 15px;
  }

  .check input {
    width: 20px;
    height: 20px;
    accent-color: var(--ink);
  }

  .ok-line {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: 0 0 8px;
  }

  .ok-line :global(svg) {
    flex: none;
    margin-top: 2px;
    color: var(--moss);
  }

  .check.big {
    align-items: flex-start;
    padding: 12px 14px;
    border-radius: 14px;
    background: var(--surface-2);
  }

  .check.big span {
    display: grid;
  }

  .check.big em {
    font-style: normal;
    font-size: 13px;
    color: var(--muted);
  }

  .nets {
    list-style: none;
    padding: 0;
    margin: 0 0 8px;
    border-radius: 14px;
    border: 1px solid var(--line);
    background: var(--bg);
    max-height: 280px;
    overflow: auto;
  }

  .nets li + li {
    border-top: 1px solid var(--line);
  }

  .nets label {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 50px;
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

  details {
    margin: 10px 0;
  }

  summary {
    cursor: pointer;
    font-weight: 600;
    color: var(--muted);
  }

  .three {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 8px;
  }

  .two .input {
    width: 100%;
  }

  .two {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
  }

  .host {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--muted);
    font-weight: 600;
  }

  .host input {
    flex: 1;
    min-width: 0;
  }

  .howto {
    padding-left: 22px;
    display: grid;
    gap: 8px;
    margin: 0 0 16px;
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
    background: var(--bg);
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
    margin-bottom: 4px;
  }

  .tz {
    margin-top: 12px;
  }

  .recap {
    margin: 0 0 16px;
    border-radius: 16px;
    border: 1px solid var(--line);
    overflow: hidden;
  }

  .recap div {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 16px;
  }

  .recap div + div {
    border-top: 1px solid var(--line);
  }

  .recap dt {
    color: var(--muted);
  }

  .recap dd {
    margin: 0;
    font-weight: 600;
    text-align: right;
  }

  .nav {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    margin-top: auto;
    padding-top: 26px;
  }

  .nav .right {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .nav .btn {
    min-width: 120px;
    justify-content: center;
  }

  .link {
    color: var(--muted);
    font-weight: 600;
  }

  /* Téléphone : les étapes deviennent une barre en haut, les boutons restent en bas. */
  @media (max-width: 760px) {
    .installer {
      grid-template-columns: 1fr;
      margin: 0 -16px;
      border-radius: 0;
      box-shadow: none;
      background: transparent;
      min-height: calc(100vh - 90px);
    }

    .steps {
      padding: 4px 16px 12px;
      background: transparent;
      border: 0;
    }

    .brand {
      margin-bottom: 12px;
    }

    .steps ol {
      display: none;
    }

    .mobile {
      display: grid;
      gap: 8px;
      font-size: 14px;
      color: var(--muted);
    }

    .mobile strong {
      color: var(--ink);
    }

    .bar {
      height: 4px;
      border-radius: 2px;
      background: var(--line);
      overflow: hidden;
    }

    .bar i {
      display: block;
      height: 100%;
      background: var(--ink);
      transition: width 0.3s ease;
    }

    .page {
      padding: 8px 16px calc(20px + env(safe-area-inset-bottom));
    }

    h1 {
      font-size: 28px;
    }

    .three {
      grid-template-columns: 1fr;
    }

    .nav {
      position: sticky;
      bottom: 0;
      margin: auto -16px 0;
      padding: 12px 16px calc(12px + env(safe-area-inset-bottom));
      background: var(--bg);
      border-top: 1px solid var(--line);
    }
  }
</style>
