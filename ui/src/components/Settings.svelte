<script>
  // Réglages rangés en rubriques : une page courte par sujet plutôt qu'une page sans fin.
  import Icon from './Icon.svelte';
  import SetSystem from './settings/SetSystem.svelte';
  import SetNotifications from './settings/SetNotifications.svelte';
  import SetAlerts from './settings/SetAlerts.svelte';
  import SetLocation from './settings/SetLocation.svelte';
  import SetRemote from './settings/SetRemote.svelte';
  import SetAccounts from './settings/SetAccounts.svelte';
  import SetPresence from './settings/SetPresence.svelte';
  import SetNetwork from './settings/SetNetwork.svelte';
  import SetHome from './settings/SetHome.svelte';
  import SetIntegrations from './settings/SetIntegrations.svelte';
  import SetBackup from './settings/SetBackup.svelte';
  import SetSecurity from './settings/SetSecurity.svelte';
  import SetJournal from './settings/SetJournal.svelte';
  import PairingSheet from './PairingSheet.svelte';
  import { describe as alarmView, isConfigured } from '../lib/security.js';
  import { home, isAdmin, isMember } from '../lib/store.svelte.js';

  let { page = '', onnavigate } = $props();

  const zigbee = $derived(home.integrations.zigbee2mqtt?.status);
  const ZSTATUS = { connected: 'Connecté', starting: 'Connexion…', disconnected: 'Injoignable', failed: 'En échec', stopped: 'Désactivé' };

  let pairing = $state(false);
  const alarmHint = $derived.by(() => {
    const sec = home.security;
    if (!sec || !isConfigured(sec.config)) return 'Alarme à configurer';
    return alarmView(sec.status, home.now).title;
  });

  const me = $derived(home.me?.user);
  const ALL_PAGES = $derived([
    {
      id: 'comptes',
      label: 'Comptes',
      icon: 'user',
      hint: home.me?.open ? 'Heim n’est pas protégé : créez votre compte' : me ? `${me.name}, se déconnecter, mot de passe` : 'Comptes de la maison',
      all: true,
    },
    { id: 'securite', label: 'Sécurité', icon: 'shield', hint: alarmHint },
    { id: 'presence', label: 'Présence', icon: 'motion', hint: 'Qui est à la maison (Wi-Fi)' },
    { id: 'maison', label: 'Maison', icon: 'house', hint: home.layout?.rooms?.length ? `${home.layout.name || 'La maison'} · ${home.layout.rooms.length} pièce(s)` : 'Étages et pièces' },
    { id: 'integrations', label: 'Intégrations', icon: 'plug', hint: integrationsHint },
    { id: 'reseau', label: 'Réseau', icon: 'link', hint: 'Wi-Fi du Raspberry Pi' },
    { id: 'notifications', label: 'Notifications', icon: 'bell', hint: 'Sur ce téléphone, et les autres', all: true },
    { id: 'alertes', label: 'Alertes', icon: 'battery', hint: 'Piles faibles, appareils muets, fuites' },
    { id: 'lieu', label: 'Lieu', icon: 'sun', hint: 'Pour le lever et le coucher du soleil' },
    { id: 'acces', label: 'Accès à distance', icon: 'link', hint: 'Heim depuis dehors, avec Tailscale' },
    { id: 'journal', label: 'Journal', icon: 'history', hint: 'Tout ce qui s’est passé, et qui l’a fait', member: true },
    { id: 'sauvegarde', label: 'Sauvegarde', icon: 'drop', hint: 'Chaque nuit, télécharger, restaurer' },
    { id: 'systeme', label: 'Système', icon: 'sensor', hint: 'Version, adresse, mémoire' },
  ]);

  // Membres et invités ne voient que leur compte et les notifications de leur téléphone.
  const PAGES = $derived(isAdmin() ? ALL_PAGES : ALL_PAGES.filter((p) => p.all || (p.member && isMember())));
  const [root, detail = ''] = $derived(page.split('/'));
  const current = $derived(PAGES.find((p) => p.id === root));
  const integrationsHint = $derived(
    ['zigbee2mqtt', 'tuya', 'matter']
      .filter((k) => home.integrations[k])
      .map((k) => `${{ zigbee2mqtt: 'Zigbee', tuya: 'Tuya', matter: 'Matter' }[k]} : ${ZSTATUS[home.integrations[k].status] ?? home.integrations[k].status}`)
      .join(' · ') || 'Zigbee, Tuya, Matter',
  );
</script>

{#if !current}
  <h1>Réglages</h1>
  <ul class="hub">
    {#each PAGES as p (p.id)}
      <li>
        <button onclick={() => onnavigate(p.id)}>
          <span class="ic"><Icon name={p.icon} size={20} /></span>
          <span class="txt"><strong>{p.label}</strong><span>{p.hint}</span></span>
          <span class="chev" aria-hidden="true">›</span>
        </button>
      </li>
    {/each}
  </ul>
{:else}
  {#if detail}
    <button class="back" onclick={() => onnavigate(root)}>‹ {current.label}</button>
  {:else}
    <button class="back" onclick={() => onnavigate('')}>‹ Réglages</button>
    <h1>{current.label}</h1>
  {/if}
  <div class="page">
    {#if root === 'maison'}
      <SetHome />
    {:else if root === 'integrations'}
      <SetIntegrations {detail} {onnavigate} />
    {:else if root === 'reseau'}
      <SetNetwork />
    {:else if root === 'securite'}
      <SetSecurity />
    {:else if root === 'notifications'}
      <SetNotifications />
    {:else if root === 'alertes'}
      <SetAlerts />
    {:else if root === 'lieu'}
      <SetLocation />
    {:else if root === 'acces'}
      <SetRemote />
    {:else if root === 'comptes'}
      <SetAccounts />
    {:else if root === 'presence'}
      <SetPresence />
    {:else if root === 'journal'}
      <SetJournal />
    {:else if root === 'sauvegarde'}
      <SetBackup />
    {:else if root === 'systeme'}
      <SetSystem />
    {/if}
  </div>
{/if}

{#if pairing}
  <PairingSheet onclose={() => (pairing = false)} />
{/if}

<style>
  h1 {
    margin: 0 0 12px;
    font-size: 40px;
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .page :global(h2) {
    font-size: 20px;
    margin: 8px 0 8px;
  }

  .page :global(.muted) {
    color: var(--muted);
  }

  .hub {
    list-style: none;
    margin: 0;
    padding: 0;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
    overflow: hidden;
  }

  .hub li + li {
    border-top: 1px solid var(--line);
  }

  .hub button {
    display: flex;
    align-items: center;
    gap: 14px;
    width: 100%;
    min-height: 68px;
    padding: 10px 16px;
    text-align: left;
  }

  .ic {
    flex: none;
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    border-radius: 12px;
    background: var(--surface-2);
    color: var(--muted);
  }

  .txt {
    flex: 1;
    display: grid;
    min-width: 0;
  }

  .txt span {
    font-size: 14px;
    color: var(--muted);
  }

  .chev {
    font-size: 26px;
    color: var(--muted);
  }

  .back {
    min-height: 40px;
    color: var(--muted);
    font-weight: 600;
    margin-bottom: 4px;
  }
</style>
