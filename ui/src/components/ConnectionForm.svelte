<script>
  import { tr } from '../lib/i18n.js';
  // Connexion à Zigbee2MQTT, sans variables d'environnement. Appliquée tout de suite.
  import { onMount } from 'svelte';
  import { api, home, toast } from '../lib/store.svelte.js';

  let host = $state('');
  let port = $state(1883);
  let username = $state('');
  let password = $state('');
  let hasPassword = $state(false);
  let baseTopic = $state('zigbee2mqtt');
  let enabled = $state(true);
  let saving = $state(false);
  let advanced = $state(false);

  onMount(async () => {
    try {
      const s = await api('/settings/zigbee2mqtt');
      if (s) {
        host = s.host ?? '';
        port = s.port ?? 1883;
        username = s.username ?? '';
        hasPassword = !!s.has_password;
        baseTopic = s.base_topic ?? 'zigbee2mqtt';
        enabled = s.enabled !== false;
      }
    } catch {
      /* pas encore configuré */
    }
  });

  const status = $derived(home.integrations.zigbee2mqtt);
  const STATUS = {
    connected: tr("Connecté"),
    starting: tr("Connexion…"),
    disconnected: tr("Injoignable, nouvel essai automatique"),
    failed: tr("En échec, nouvel essai automatique"),
    stopped: tr("Désactivé"),
  };

  async function save(e) {
    e.preventDefault();
    saving = true;
    try {
      const body = { enabled, host: host.trim(), port: Number(port) || 1883, username: username.trim() || null, base_topic: baseTopic.trim() || 'zigbee2mqtt' };
      if (password) body.password = password;
      const r = await api('/settings/zigbee2mqtt', { method: 'PUT', body });
      hasPassword = !!r.has_password;
      password = '';
      toast(enabled ? tr("Connexion enregistrée, Heim se connecte…") : tr("Zigbee désactivé"));
    } catch (err) {
      toast(err.message, 'alert');
    } finally {
      saving = false;
    }
  }
</script>

<form onsubmit={save}>
  {#if status}
    <p class="status" class:ok={status.status === 'connected'} class:bad={['failed', 'disconnected'].includes(status.status)}>
      <span class="led"></span>
      {STATUS[status.status] ?? status.status}
      {#if status.detail && status.status !== 'connected'}<small>{status.detail}</small>{/if}
    </p>
  {/if}

  <label class="field">
    <span>{tr("Adresse du broker MQTT")}</span>
    <input class="input" bind:value={host} placeholder="192.168.1.50" autocomplete="off" inputmode="url" />
  </label>
  <div class="two">
    <label class="field">
      <span>{tr("Utilisateur")}</span>
      <input class="input" bind:value={username} autocomplete="username" placeholder={tr("heim")} />
    </label>
    <label class="field">
      <span>{tr("Mot de passe")}</span>
      <input class="input" type="password" bind:value={password} autocomplete="current-password" placeholder={hasPassword ? tr("inchangé") : ''} />
    </label>
  </div>

  {#if advanced}
    <div class="two">
      <label class="field">
        <span>{tr("Port")}</span>
        <input class="input num" type="number" bind:value={port} min="1" max="65535" />
      </label>
      <label class="field">
        <span>{tr("Topic de Zigbee2MQTT")}</span>
        <input class="input" bind:value={baseTopic} />
      </label>
    </div>
    <label class="check"><input type="checkbox" bind:checked={enabled} /> {tr("Connexion active")}</label>
  {:else}
    <button type="button" class="more" onclick={() => (advanced = true)}>{tr("Réglages avancés")}</button>
  {/if}

  <button class="btn" type="submit" disabled={saving || (enabled && !host.trim())}>{tr("Enregistrer et se connecter")}</button>
</form>

<style>
  .two {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 0 12px;
  }

  .status {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 0 0 16px;
    font-weight: 600;
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

  .more {
    color: var(--muted);
    font-size: 14px;
    margin: -6px 0 18px;
    min-height: 36px;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 18px;
  }
</style>
