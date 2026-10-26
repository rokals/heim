<script>
  import { tr } from '../lib/i18n.js';
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
  import SetLook from './settings/SetLook.svelte';
  import SetScreens from './settings/SetScreens.svelte';
  import PairingSheet from './PairingSheet.svelte';
  import { describe as alarmView, isConfigured } from '../lib/security.js';
  import { home, isAdmin, isMember } from '../lib/store.svelte.js';

  let { page = '', onnavigate } = $props();

  const zigbee = $derived(home.integrations.zigbee2mqtt?.status);
  const ZSTATUS = { connected: tr("Connecté"), starting: tr("Connexion…"), disconnected: tr("Injoignable"), failed: tr("En échec"), stopped: tr("Désactivé") };

  let pairing = $state(false);
  const alarmHint = $derived.by(() => {
    const sec = home.security;
    if (!sec || !isConfigured(sec.config)) return tr("Alarme à configurer");
    return alarmView(sec.status, home.now).title;
  });

  const me = $derived(home.me?.user);
  const ALL_PAGES = $derived([
    {
      id: 'comptes',
      label: tr("Comptes"),
      icon: 'user',
      hint: home.me?.open ? tr("Heim n’est pas protégé : créez votre compte") : me ? tr("{name}, se déconnecter, mot de passe", { name: me.name }) : tr("Comptes de la maison"),
      all: true,
    },
    { id: 'apparence', label: tr("Apparence"), icon: 'sun', hint: tr("Thème clair ou sombre, couleur, taille du texte"), all: true },
    { id: 'ecrans', label: tr("Écrans"), icon: 'grid', hint: tr("Tablettes et téléphones : tableau, limites, mode tablette murale") },
    { id: 'securite', label: tr("Sécurité"), icon: 'shield', hint: alarmHint },
    { id: 'presence', label: tr("Présence"), icon: 'motion', hint: tr("Qui est à la maison (Wi-Fi)") },
    { id: 'maison', label: tr("Maison"), icon: 'house', hint: home.layout?.rooms?.length ? tr("{v} · {length} pièce(s)", { v: home.layout.name || tr("La maison"), length: home.layout.rooms.length }) : tr("Étages et pièces") },
    { id: 'integrations', label: tr("Connexions"), icon: 'plug', hint: integrationsHint },
    { id: 'reseau', label: tr("Réseau"), icon: 'link', hint: tr("Wi-Fi du Raspberry Pi") },
    { id: 'notifications', label: tr("Notifications"), icon: 'bell', hint: tr("Sur ce téléphone, et les autres"), all: true },
    { id: 'alertes', label: tr("Alertes"), icon: 'battery', hint: tr("Piles faibles, appareils muets, fuites") },
    { id: 'lieu', label: tr("Lieu"), icon: 'sun', hint: tr("Pour le lever et le coucher du soleil") },
    { id: 'acces', label: tr("Accès à distance"), icon: 'link', hint: tr("Heim depuis dehors, avec Tailscale") },
    { id: 'journal', label: tr("Journal"), icon: 'history', hint: tr("Tout ce qui s’est passé, et qui l’a fait"), member: true },
    { id: 'sauvegarde', label: tr("Sauvegarde"), icon: 'drop', hint: tr("Chaque nuit, télécharger, restaurer") },
    { id: 'systeme', label: tr("Système"), icon: 'sensor', hint: tr("Version, adresse, mémoire") },
  ]);

  // Membres et invités ne voient que leur compte et les notifications de leur téléphone.
  const PAGES = $derived(isAdmin() ? ALL_PAGES : ALL_PAGES.filter((p) => p.all || (p.member && isMember())));
  const [root, detail = ''] = $derived(page.split('/'));
  const current = $derived(PAGES.find((p) => p.id === root));
  const integrationsHint = $derived(
    ['zigbee2mqtt', 'tuya', 'matter']
      .filter((k) => home.integrations[k])
      .map((k) => `${{ zigbee2mqtt: tr("Zigbee"), tuya: tr("Tuya"), matter: tr("Matter") }[k]} : ${ZSTATUS[home.integrations[k].status] ?? home.integrations[k].status}`)
      .join(' · ') || tr("Zigbee, Tuya, Matter"),
  );
</script>

{#if !current}
  <h1>{tr("Réglages")}</h1>
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
    <button class="back" onclick={() => onnavigate('')}>{tr("‹ Réglages")}</button>
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
    {:else if root === 'apparence'}
      <SetLook />
    {:else if root === 'ecrans'}
      <SetScreens />
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
