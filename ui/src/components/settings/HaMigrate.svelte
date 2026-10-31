<script>
  import { tr } from '../../lib/i18n.js';
  // Reprendre une maison Home Assistant, étape par étape : chaque étape est un bloc repliable
  // dont la pastille passe au vert quand c'est fait (au rouge, avec la raison, si ça échoue),
  // et l'étape suivante s'ouvre toute seule. Heim demande sa sauvegarde à Zigbee2MQTT (par HA
  // et un jeton, ou directement par le broker MQTT), lit les pièces, puis on déplace la clé.
  import Icon from '../Icon.svelte';
  import Fold from '../Fold.svelte';
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
  // L'erreur de chaque étape, affichée dans l'étape elle-même.
  let err = $state({ check: '', import: '', stop: '', mqtt: '' });
  let open = $state([true, false, false, false]);

  const body = () => ({ url: url.trim(), token: token.trim(), base_topic: topic.trim() || null });
  const next = (i) => (open = open.map((_, k) => k === i));

  async function act(kind, fn) {
    busy = kind;
    err[kind] = '';
    try {
      await fn();
    } catch (e) {
      err[kind] = e.message;
      toast(e.message, 'alert');
    }
    busy = '';
  }

  const check = (e) => {
    e?.preventDefault();
    return act('check', async () => {
      ha = await api('/migrate/ha/check', { method: 'POST', body: body() });
      try {
        localStorage.setItem('heim_ha_url', url.trim());
      } catch {}
      next(1);
    });
  };

  const run = () =>
    act('import', async () => {
      result = await api('/migrate/ha/import', { method: 'POST', body: body() });
      if (result.addon) ha = { ...ha, addon: result.addon };
      toast(tr("Repris : {n} appareil(s)", { n: result.imported?.devices ?? 0 }));
      ondone?.(result);
      next(2);
    });

  const stopAddon = () => {
    if (!confirm(tr("Arrêter Zigbee2MQTT dans Home Assistant ? Les appareils Zigbee n'y répondront plus."))) return;
    return act('stop', async () => {
      await api('/migrate/ha/stop_addon', { method: 'POST', body: { ...body(), slug: ha.addon.slug } });
      stopped = true;
      toast(tr("Zigbee2MQTT arrêté dans Home Assistant"));
      next(3);
    });
  };

  const runMqtt = (e) => {
    e.preventDefault();
    return act('mqtt', async () => {
      result = await api('/migrate/mqtt/import', {
        method: 'POST',
        body: { host: mq.host.trim(), port: Number(mq.port) || 1883, username: mq.username || null, password: mq.password || null, base_topic: topic.trim() || null },
      });
      toast(tr("Repris : {n} appareil(s)", { n: result.imported?.devices ?? 0 }));
      ondone?.(result);
    });
  };

  const tone = (done, kind) => (done ? 'ok' : err[kind] ? 'bad' : busy === kind ? 'wait' : '');
</script>

{#snippet failed(kind)}
  {#if err[kind]}<p class="err"><Icon name="alert" size={16} /> {err[kind]}</p>{/if}
{/snippet}

<div class="mig">
  {#if !viaMqtt}
    <Fold step={1} title={tr("Se connecter à Home Assistant")} tone={tone(ha, 'check')} bind:shown={open[0]}
      sub={ha ? `${tr("Connecté")}${ha.version ? ` · Home Assistant ${ha.version}` : ''} · ${tr("{n} pièce(s)", { n: ha.rooms?.length ?? 0 })}` : err.check ? tr("Échec") : tr("Adresse et jeton d'accès")}>
      <form onsubmit={check}>
        <input class="input" bind:value={url} placeholder="homeassistant.local:8123" autocomplete="off" aria-label={tr("Adresse de Home Assistant")} />
        <span class="muted small">{tr("Le nom homeassistant.local ou son adresse IP (192.168.1.10:8123).")}</span>
        <input class="input" type="password" bind:value={token} placeholder={tr("Jeton d'accès longue durée")} autocomplete="off" aria-label={tr("Jeton d'accès longue durée")} />
        <span class="muted small">{tr("Dans Home Assistant : votre profil (en bas à gauche) › Sécurité › Jetons d'accès longue durée › Créer un jeton. Le compte doit être administrateur. Heim ne garde pas ce jeton.")}</span>
        <label class="field"><span>{tr("Sujet de base de Zigbee2MQTT")}</span><input class="input" bind:value={topic} /></label>
        <button class="btn" class:okbtn={ha} type="submit" disabled={busy || !url.trim() || !token.trim()}>
          {#if busy === 'check'}{tr("Connexion…")}{:else if ha}<Icon name="check" size={18} /> {tr("Connecté")}{:else}{tr("Se connecter")}{/if}
        </button>
        {@render failed('check')}
      </form>
    </Fold>

    <Fold step={2} title={tr("Importer les appareils et les pièces")} tone={tone(result, 'import')} bind:shown={open[1]}
      sub={result ? tr("{n} appareil(s) repris, {r} pièce(s) ajoutée(s)", { n: result.imported?.devices ?? 0, r: result.rooms_added ?? 0 }) : busy === 'import' ? tr("Sauvegarde en cours…") : err.import ? tr("Échec") : !ha ? tr("Après l'étape 1") : tr("Clé encore sur Home Assistant")}>
      <span class="muted small">{tr("Heim demande sa sauvegarde à Zigbee2MQTT (clé réseau, appareils). Avec une seule clé, laissez-la branchée sur Home Assistant pour l'instant : si Zigbee2MQTT y est arrêté, Heim le relance le temps de la sauvegarde.")}</span>
      <button class="btn" class:okbtn={result} onclick={run} disabled={busy || !ha}>
        {#if busy === 'import'}{tr("Sauvegarde en cours…")}{:else if result}<Icon name="check" size={18} /> {tr("Importé")}{:else}{tr("Importer")}{/if}
      </button>
      {@render failed('import')}
    </Fold>

    <Fold step={3} title={tr("Arrêter Zigbee2MQTT dans Home Assistant")} tone={tone(stopped, 'stop')} bind:shown={open[2]}
      sub={stopped ? tr("Arrêté") : !result ? tr("Après l'étape 2") : tr("Une clé ne sert qu'à un seul Zigbee2MQTT")}>
      {#if ha?.addon}
        <button class="btn" class:okbtn={stopped} onclick={stopAddon} disabled={busy || !result || stopped}>
          {#if busy === 'stop'}{tr("Arrêt…")}{:else if stopped}<Icon name="check" size={18} /> {tr("Arrêté")}{:else}{tr("Arrêter le module {name}", { name: ha.addon.name })}{/if}
        </button>
        <span class="muted small">{tr("Pensez aussi à couper son démarrage automatique dans Home Assistant (Paramètres › Modules complémentaires).")}</span>
      {:else}
        <span class="muted small">{tr("Arrêtez-le vous-même (module complémentaire ou conteneur Docker) : une clé et un réseau Zigbee ne servent qu'à un seul Zigbee2MQTT à la fois.")}</span>
        <button class="btn" class:okbtn={stopped} onclick={() => ((stopped = true), next(3))} disabled={!result || stopped}>{#if stopped}<Icon name="check" size={18} /> {/if}{tr("C'est fait")}</button>
      {/if}
      {@render failed('stop')}
    </Fold>

    <Fold step={4} title={tr("Déplacer la clé Zigbee")} tone="" bind:shown={open[3]} sub={stopped ? tr("Puis démarrer Zigbee sur la box") : tr("Après l'étape 3")}>
      <span class="muted small">{where === 'below' ? tr("Débranchez la clé de la machine Home Assistant, branchez-la sur la box, choisissez-la ci-dessous et démarrez Zigbee sur la box. Les appareils reviennent sans réappairage (les appareils sur pile peuvent mettre quelques minutes).") : tr("Débranchez la clé de la machine Home Assistant, branchez-la sur la box, choisissez-la ci-dessus et démarrez Zigbee sur la box. Les appareils reviennent sans réappairage (les appareils sur pile peuvent mettre quelques minutes).")}</span>
    </Fold>

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
      <button class="btn" class:okbtn={result} type="submit" disabled={busy || !mq.host.trim()}>
        {#if busy === 'mqtt'}{tr("Sauvegarde en cours…")}{:else if result}<Icon name="check" size={18} /> {tr("Importé")}{:else}{tr("Importer")}{/if}
      </button>
      {@render failed('mqtt')}
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

  .btn {
    justify-self: start;
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .btn.okbtn,
  .btn.okbtn:disabled {
    background: var(--moss);
    color: var(--surface);
    opacity: 1;
  }

  .err {
    display: flex;
    gap: 6px;
    margin: 0;
    color: var(--alert);
    font-size: 14px;
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

</style>
