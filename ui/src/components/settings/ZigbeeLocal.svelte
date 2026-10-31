<script>
  import { tr } from '../../lib/i18n.js';
  import { copyText } from '../../lib/clipboard.js';
  // Zigbee2MQTT sur la box : la clé USB, démarrer / arrêter, reprendre un Zigbee2MQTT
  // existant (Home Assistant), l'interface web de Zigbee2MQTT et son journal.
  // compact : seulement la clé et le bouton (assistant, accueil vide).
  import { onDestroy, onMount } from 'svelte';
  import Icon from '../Icon.svelte';
  import HaMigrate from './HaMigrate.svelte';
  import Fold from '../Fold.svelte';
  import { api, toast } from '../../lib/store.svelte.js';

  let { compact = false, onstarted = null } = $props();

  let st = $state(null);
  let port = $state('');
  let adapter = $state('');
  let busy = $state(false);
  let files = $state({ configuration: null, database: null, coordinator_backup: null });
  let importing = $state(false);
  let timer;

  const ADAPTERS = [
    ['', tr("Automatique")],
    ['ember', 'ember (Sonoff ZBDongle-E, SkyConnect, EFR32)'],
    ['zstack', 'zstack (Texas Instruments : Sonoff ZBDongle-P, CC2652)'],
    ['deconz', 'deconz (ConBee, RaspBee)'],
    ['zigate', 'zigate (ZiGate)'],
    ['zboss', 'zboss (nRF52840)'],
  ];

  async function load(first = false) {
    try {
      const s = await api('/zigbee/local');
      st = s;
      if (first) {
        port = s.port ?? '';
        adapter = s.adapter ?? '';
      }
    } catch {
      /* hors ligne : on réessaie au prochain tour */
    }
  }

  onMount(() => {
    load(true);
    timer = setInterval(load, 3000);
  });
  onDestroy(() => clearInterval(timer));

  const STATE = $derived({
    active: st?.bridge ? tr("Zigbee2MQTT tourne et répond") : tr("Zigbee2MQTT démarre…"),
    activating: tr("Zigbee2MQTT démarre…"),
    reloading: tr("Zigbee2MQTT redémarre…"),
    deactivating: tr("Arrêt…"),
    failed: tr("Zigbee2MQTT s'est arrêté sur une erreur"),
    inactive: tr("Arrêté"),
    missing: tr("Pas installé sur cette box"),
  });
  const tone = $derived(
    !st ? 'off' : st.state === 'active' && st.bridge ? 'ok' : st.state === 'failed' ? 'bad' : st.state === 'inactive' ? 'off' : 'wait',
  );
  // La dernière ligne d'erreur du journal, quand Heim ne sait pas l'expliquer.
  const lastError = $derived([...(st?.log ?? [])].reverse().find((l) => /error|erreur|failed|exception/i.test(l)) ?? '');
  // Le pilote vraiment utilisé (choisi, ou dans la configuration), sinon celui deviné au nom.
  const driverOf = (d) => (d.path === chosen ? adapter || st?.config?.adapter || d.adapter : d.adapter);
  const label = (d) => d.model || d.name.replace(/^usb-/, '').replace(/-if\d+(-port\d+)?$/, '').replaceAll('_', ' ');
  const chosen = $derived(port || st?.dongles?.[0]?.path || '');

  async function send(body, done) {
    busy = true;
    try {
      st = await api('/zigbee/local', { method: 'PUT', body });
      if (done) toast(done);
      if (body.enabled) onstarted?.();
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  function start() {
    // Clé jamais utilisée par ce Zigbee2MQTT : si elle vient de Home Assistant, démarrer
    // maintenant lui ferait un nouveau réseau (appareils à réappairer). On prévient d'abord.
    if (!st?.network_known && !st?.config?.devices && !confirm(tr("Cette clé servait-elle à Home Assistant ? Alors annulez et faites d'abord « Reprendre les appareils de Home Assistant » : démarrer maintenant créerait un nouveau réseau sur la clé et il faudrait réappairer tous les appareils. Démarrer avec un nouveau réseau ?"))) return;
    return startNow();
  }

  async function copyLog() {
    try {
      await copyText(st.log.join('\n'));
      toast(tr("Journal copié"));
    } catch {
      toast(tr("Copie impossible"), 'alert');
    }
  }

  const startNow = () => send({ enabled: true, port: chosen || null, adapter: adapter || null, frontend: !!st?.frontend }, tr("Zigbee2MQTT démarre sur la box"));
  const stop = () => send({ enabled: false, port: port || null, adapter: adapter || null, frontend: !!st?.frontend }, tr("Zigbee2MQTT arrêté"));
  const setFrontend = (on) => send({ enabled: st.enabled, port: port || null, adapter: adapter || null, frontend: on });

  async function restart() {
    busy = true;
    try {
      st = await api('/zigbee/local/restart', { method: 'POST' });
      toast(tr("Zigbee2MQTT redémarre"));
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  function pick(kind, e) {
    files[kind] = e.currentTarget.files?.[0] ?? null;
  }

  async function doImport() {
    importing = true;
    try {
      const body = { configuration: await files.configuration.text() };
      if (files.database) body.database = await files.database.text();
      if (files.coordinator_backup) body.coordinator_backup = await files.coordinator_backup.text();
      const r = await api('/zigbee/local/import', { method: 'POST', body });
      st = r.status;
      if (r.imported?.adapter && !adapter) adapter = r.imported.adapter;
      toast(tr("Repris : {n} appareil(s)", { n: r.imported?.devices ?? 0 }));
      files = { configuration: null, database: null, coordinator_backup: null };
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      importing = false;
    }
  }

  const frontendUrl = $derived(st?.frontend ? `http://${location.hostname}:${st.frontend_port}/` : null);
</script>

{#if !st}
  <p class="muted">{tr("Chargement…")}</p>
{:else if !st.installed}
  <div class="note">
    <p><strong>{tr("Zigbee2MQTT n'est pas installé sur cette box.")}</strong></p>
    <p class="muted">{tr("L'image SD de Heim l'installe. Sur un Raspberry Pi OS déjà en place, relancez l'installation avec HEIM_ZIGBEE=1. Sinon, branchez Heim sur un Zigbee2MQTT qui tourne ailleurs.")}</p>
  </div>
{:else}
  <p class="status {tone}"><span class="led"></span>{STATE[st.state] ?? st.state}{#if st.version}<small>Zigbee2MQTT {st.version}</small>{/if}</p>
  {#if tone === 'bad'}
    {#if st.hint}
      <p class="why">{tr(st.hint)}</p>
    {/if}
    {#if st.error || lastError}
      <p class="why"><code>{st.error || lastError}</code></p>
    {/if}
  {/if}

  <div class="field">
    <span>{tr("Clé Zigbee")}</span>
    {#if st.dongles.length}
      <div class="dongles" role="radiogroup" aria-label={tr("Clé Zigbee")}>
        {#each st.dongles as d (d.path)}
          <button role="radio" aria-checked={chosen === d.path} class:sel={chosen === d.path} onclick={() => (port = d.path)} disabled={busy}>
            <Icon name="link" size={18} />
            <span class="txt"><strong>{label(d)}</strong><small>{driverOf(d) ? tr("pilote {v}", { v: driverOf(d) }) : tr("pilote à choisir")}</small></span>
            {#if chosen === d.path}<Icon name="check" size={18} />{/if}
          </button>
        {/each}
      </div>
    {:else}
      <p class="muted">{tr("Aucune clé détectée. Branchez la clé Zigbee (Sonoff ZBDongle-E…) sur un port USB : elle apparaît ici toute seule.")}</p>
      {#if port}<p class="muted small">{tr("Réglée : {v}", { v: port })}</p>{/if}
    {/if}
  </div>

  {#if !compact}
    <Fold icon="sliders" title={tr("Réglages avancés")} sub={tr("Pilote, chemin de la clé, interface web")}>
      <label class="field">
        <span>{tr("Pilote de la clé")}</span>
        <select class="input" bind:value={adapter} disabled={busy}>
          {#each ADAPTERS as [id, name] (id)}<option value={id}>{name}</option>{/each}
        </select>
      </label>
      <label class="field">
        <span>{tr("Chemin de la clé (si elle n'est pas listée)")}</span>
        <input class="input" bind:value={port} placeholder="/dev/ttyACM0" autocomplete="off" disabled={busy} />
      </label>
      <label class="check">
        <input type="checkbox" checked={st.frontend} disabled={busy || !st.enabled} onchange={(e) => setFrontend(e.currentTarget.checked)} />
        {tr("Interface web de Zigbee2MQTT (port {port})", { port: st.frontend_port })}
      </label>
      {#if frontendUrl}
        <p class="muted small">
          <a href={frontendUrl} target="_blank" rel="noopener">{frontendUrl}</a> · {tr("jeton :")} <code>{st.frontend_token}</code>
        </p>
      {/if}
    </Fold>
  {/if}

  <div class="actions">
    {#if st.enabled}
      <button class="btn" onclick={start} disabled={busy}>{tr("Enregistrer et redémarrer")}</button>
      {#if !compact}<button class="btn quiet" onclick={restart} disabled={busy}><Icon name="restart" size={18} /> {tr("Redémarrer")}</button>{/if}
      <button class="btn quiet" onclick={stop} disabled={busy}>{tr("Arrêter")}</button>
    {:else}
      <button class="btn" onclick={start} disabled={busy || !chosen}>{tr("Démarrer Zigbee sur la box")}</button>
    {/if}
  </div>

  {#if !compact}
    <Fold icon="house" title={tr("Reprendre les appareils de Home Assistant")} sub={st.network_known ? tr("Réseau Zigbee en place") : tr("À faire avant de démarrer, si la clé vient de Home Assistant")} shown={!st.network_known}>
      <HaMigrate where="above" ondone={() => load()} />
    </Fold>

    <Fold icon="copy" title={tr("Reprendre à partir des fichiers")} sub={tr("configuration.yaml, database.db, coordinator_backup.json")}>
      <p class="muted">
        {tr("Pour garder vos appareils sans les réappairer : récupérez les fichiers du dossier zigbee2mqtt de l'ancienne installation (dans Home Assistant : /config/zigbee2mqtt ou /homeassistant/zigbee2mqtt), arrêtez-y Zigbee2MQTT, puis branchez la même clé sur la box.")}
      </p>
      <label class="field">
        <span>configuration.yaml</span>
        <input type="file" accept=".yaml,.yml" onchange={(e) => pick('configuration', e)} />
      </label>
      <label class="field">
        <span>database.db <small class="muted">{tr("(conseillé)")}</small></span>
        <input type="file" accept=".db,.json,.txt" onchange={(e) => pick('database', e)} />
      </label>
      <label class="field">
        <span>coordinator_backup.json <small class="muted">{tr("(conseillé)")}</small></span>
        <input type="file" accept=".json" onchange={(e) => pick('coordinator_backup', e)} />
      </label>
      <button class="btn" onclick={doImport} disabled={importing || !files.configuration}>{tr("Reprendre ces fichiers")}</button>
      {#if st.config?.devices}<p class="muted small">{tr("Configuration actuelle : {n} appareil(s), canal {c}.", { n: st.config.devices, c: st.config.channel ?? 11 })}</p>{/if}
    </Fold>

    {#if st.log?.length}
      <Fold icon="history" title={tr("Journal de Zigbee2MQTT")} sub={tr("{n} dernières lignes", { n: st.log.length })} tone={st.state === 'failed' ? 'bad' : ''}>
        <pre>{st.log.join('\n')}</pre>
        <button class="btn quiet" onclick={copyLog}><Icon name="copy" size={18} /> {tr("Copier le journal")}</button>
      </Fold>
    {/if}
  {/if}
{/if}

<style>
  .status {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 0 0 16px;
    font-weight: 600;
  }

  .why {
    margin: -8px 0 16px;
    font-size: 14px;
    color: var(--muted);
  }

  .why code {
    white-space: pre-wrap;
    word-break: break-word;
  }

  .status small {
    flex-basis: 100%;
    font-weight: 400;
    color: var(--muted);
  }

  .led {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--muted);
  }

  .ok .led {
    background: var(--moss);
  }

  .bad .led {
    background: var(--alert);
  }

  .wait .led {
    background: var(--ember);
  }

  .dongles {
    display: grid;
    gap: 8px;
  }

  .dongles button {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
    border-radius: 14px;
    background: var(--surface);
    box-shadow: var(--shadow);
    border: 2px solid transparent;
    text-align: left;
  }

  .dongles button.sel {
    border-color: var(--ink);
  }

  .dongles .txt {
    flex: 1;
    display: grid;
    min-width: 0;
  }

  .dongles small {
    color: var(--muted);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 16px 0;
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

  .small {
    font-size: 13px;
  }

  code {
    font-size: 12px;
    word-break: break-all;
  }

  pre {
    max-height: 260px;
    overflow: auto;
    padding: 10px;
    border-radius: 12px;
    background: var(--surface-2);
    font-size: 11px;
    line-height: 1.4;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .note {
    padding: 14px 16px;
    border-radius: 16px;
    background: var(--surface-2);
  }

  .note p {
    margin: 0 0 6px;
  }

  input[type='file'] {
    max-width: 100%;
  }
</style>
