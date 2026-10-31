<script>
  import { tr } from '../../lib/i18n.js';
  // Reprendre une maison Home Assistant : Heim demande à Zigbee2MQTT sa sauvegarde (par
  // Home Assistant et un jeton d'accès, ou directement par le broker MQTT), lit les pièces,
  // puis on déplace la clé Zigbee. Aucun appareil à réappairer.
  import Icon from '../Icon.svelte';
  import { api, toast } from '../../lib/store.svelte.js';

  // where : où se trouve le bouton « Démarrer Zigbee sur la box » ('above' ou 'below').
  let { where = 'above', ondone = null } = $props();

  let saved = '';
  try {
    saved = localStorage.getItem('heim_ha_url') || '';
  } catch {}
  let url = $state(saved || 'homeassistant.local:8123');
  let token = $state('');
  let topic = $state('zigbee2mqtt');
  let busy = $state('');
  let ha = $state(null);
  let result = $state(null);
  let stopped = $state(false);
  let viaMqtt = $state(false);
  let mq = $state({ host: '', port: 1883, username: '', password: '' });

  const body = () => ({ url: url.trim(), token: token.trim(), base_topic: topic.trim() || null });

  async function check(e) {
    e?.preventDefault();
    busy = 'check';
    try {
      ha = await api('/migrate/ha/check', { method: 'POST', body: body() });
      try {
        localStorage.setItem('heim_ha_url', url.trim());
      } catch {}
    } catch (err) {
      toast(err.message, 'alert');
    }
    busy = '';
  }

  async function run() {
    busy = 'import';
    try {
      result = await api('/migrate/ha/import', { method: 'POST', body: body() });
      if (result.addon) ha = { ...ha, addon: result.addon };
      toast(tr("Repris : {n} appareil(s)", { n: result.imported?.devices ?? 0 }));
      ondone?.(result);
    } catch (err) {
      toast(err.message, 'alert');
    }
    busy = '';
  }

  async function stopAddon() {
    if (!confirm(tr("Arrêter Zigbee2MQTT dans Home Assistant ? Les appareils Zigbee n'y répondront plus."))) return;
    busy = 'stop';
    try {
      await api('/migrate/ha/stop_addon', { method: 'POST', body: { ...body(), slug: ha.addon.slug } });
      stopped = true;
      toast(tr("Zigbee2MQTT arrêté dans Home Assistant"));
    } catch (err) {
      toast(err.message, 'alert');
    }
    busy = '';
  }

  async function runMqtt(e) {
    e.preventDefault();
    busy = 'mqtt';
    try {
      result = await api('/migrate/mqtt/import', {
        method: 'POST',
        body: { host: mq.host.trim(), port: Number(mq.port) || 1883, username: mq.username || null, password: mq.password || null, base_topic: topic.trim() || null },
      });
      toast(tr("Repris : {n} appareil(s)", { n: result.imported?.devices ?? 0 }));
      ondone?.(result);
    } catch (err) {
      toast(err.message, 'alert');
    }
    busy = '';
  }
</script>

<div class="mig">
  {#if !viaMqtt}
    <ol class="steps">
      <li class:done={ha}>
        <strong>{tr("Se connecter à Home Assistant")}</strong>
        {#if ha}
          <span class="muted"><Icon name="check" size={16} /> {tr("Connecté")}{ha.version ? ` · Home Assistant ${ha.version}` : ''} · {tr("{n} pièce(s)", { n: ha.rooms?.length ?? 0 })}</span>
        {:else}
          <form onsubmit={check}>
            <input class="input" bind:value={url} placeholder="homeassistant.local:8123" autocomplete="off" aria-label={tr("Adresse de Home Assistant")} />
            <input class="input" type="password" bind:value={token} placeholder={tr("Jeton d'accès longue durée")} autocomplete="off" aria-label={tr("Jeton d'accès longue durée")} />
            <span class="muted small">{tr("Dans Home Assistant : votre profil (en bas à gauche) › Sécurité › Jetons d'accès longue durée › Créer un jeton. Le compte doit être administrateur. Heim ne garde pas ce jeton.")}</span>
            <details class="adv">
              <summary>{tr("Avancé")}</summary>
              <label class="field"><span>{tr("Sujet de base de Zigbee2MQTT")}</span><input class="input" bind:value={topic} /></label>
            </details>
            <button class="btn" type="submit" disabled={busy || !url.trim() || !token.trim()}>{busy === 'check' ? tr("Connexion…") : tr("Se connecter")}</button>
          </form>
        {/if}
      </li>

      <li class:done={result} class:off={!ha}>
        <strong>{tr("Reprendre Zigbee2MQTT et les pièces")}</strong>
        {#if result}
          <span class="muted"><Icon name="check" size={16} /> {tr("{n} appareil(s) repris, {r} pièce(s) ajoutée(s)", { n: result.imported?.devices ?? 0, r: result.rooms_added ?? 0 })}</span>
        {:else if ha}
          <span class="muted small">{tr("Heim demande sa sauvegarde à Zigbee2MQTT (clé réseau, appareils) : il doit tourner dans Home Assistant à ce moment-là.")}</span>
          <button class="btn" onclick={run} disabled={busy}>{busy === 'import' ? tr("Sauvegarde en cours…") : tr("Reprendre")}</button>
        {/if}
      </li>

      <li class:done={stopped} class:off={!result}>
        <strong>{tr("Arrêter Zigbee2MQTT dans Home Assistant")}</strong>
        {#if result && !stopped}
          {#if ha?.addon}
            <button class="btn quiet" onclick={stopAddon} disabled={busy}>{busy === 'stop' ? tr("Arrêt…") : tr("Arrêter le module {name}", { name: ha.addon.name })}</button>
            <span class="muted small">{tr("Pensez aussi à couper son démarrage automatique dans Home Assistant (Paramètres › Modules complémentaires).")}</span>
          {:else}
            <span class="muted small">{tr("Arrêtez-le vous-même (module complémentaire ou conteneur Docker) : une clé et un réseau Zigbee ne servent qu'à un seul Zigbee2MQTT à la fois.")}</span>
            <button class="btn quiet" onclick={() => (stopped = true)}>{tr("C'est fait")}</button>
          {/if}
        {/if}
      </li>

      <li class:off={!stopped}>
        <strong>{tr("Déplacer la clé Zigbee")}</strong>
        {#if stopped}
          <span class="muted small">{where === 'below' ? tr("Débranchez la clé de la machine Home Assistant, branchez-la sur la box, choisissez-la ci-dessous et démarrez Zigbee sur la box. Les appareils reviennent sans réappairage (les appareils sur pile peuvent mettre quelques minutes).") : tr("Débranchez la clé de la machine Home Assistant, branchez-la sur la box, choisissez-la ci-dessus et démarrez Zigbee sur la box. Les appareils reviennent sans réappairage (les appareils sur pile peuvent mettre quelques minutes).")}</span>
        {/if}
      </li>
    </ol>
    <p class="muted small">{tr("Appareils Wi-Fi branchés sur le broker MQTT de Home Assistant (Tasmota, Shelly, ESPHome) : réglez-les sur l'adresse de la box, port 1883, et ouvrez le broker de Heim au réseau. Tuya se reprend dans Réglages › Connexions › Tuya.")}</p>
    <button class="link" onclick={() => (viaMqtt = true)}>{tr("Sans jeton : passer directement par le broker MQTT")}</button>
  {:else}
    <form onsubmit={runMqtt}>
      <span class="muted small">{tr("L'adresse et un compte du broker MQTT où tourne Zigbee2MQTT (dans Home Assistant : le module Mosquitto, et un utilisateur de Home Assistant). Zigbee2MQTT doit tourner.")}</span>
      <input class="input" bind:value={mq.host} placeholder={tr("Adresse (192.168.1.10)")} autocomplete="off" />
      <input class="input" type="number" bind:value={mq.port} placeholder="1883" />
      <input class="input" bind:value={mq.username} placeholder={tr("Utilisateur")} autocomplete="off" />
      <input class="input" type="password" bind:value={mq.password} placeholder={tr("Mot de passe")} autocomplete="off" />
      <label class="field"><span>{tr("Sujet de base de Zigbee2MQTT")}</span><input class="input" bind:value={topic} /></label>
      <button class="btn" type="submit" disabled={busy || !mq.host.trim()}>{busy === 'mqtt' ? tr("Sauvegarde en cours…") : tr("Reprendre")}</button>
      {#if result}<span class="muted"><Icon name="check" size={16} /> {tr("Repris : {n} appareil(s)", { n: result.imported?.devices ?? 0 })} — {tr("arrêtez maintenant Zigbee2MQTT sur l'autre machine, puis déplacez la clé sur la box.")}</span>{/if}
      <button class="link" type="button" onclick={() => (viaMqtt = false)}>{tr("Passer par Home Assistant")}</button>
    </form>
  {/if}
</div>

<style>
  .mig {
    display: grid;
    gap: 10px;
    margin: 8px 0;
  }

  .steps {
    margin: 0;
    padding-left: 22px;
    list-style: decimal;
    display: grid;
    gap: 14px;
  }

  .steps li {
    display: grid;
    gap: 6px;
  }

  .steps li.off {
    opacity: 0.5;
  }

  .steps li.done strong {
    color: var(--moss);
  }

  form {
    display: grid;
    gap: 8px;
    max-width: 460px;
  }

  .field {
    display: grid;
    gap: 4px;
  }

  .small {
    font-size: 14px;
  }

  .link {
    justify-self: start;
    text-decoration: underline;
    color: var(--muted);
  }

  .adv summary {
    cursor: pointer;
    color: var(--muted);
    font-size: 14px;
  }
</style>
