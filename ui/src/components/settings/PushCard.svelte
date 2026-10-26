<script>
  import { tr } from '../../lib/i18n.js';
  // « Recevoir les alertes sur ce téléphone, même Heim fermé. »
  import { onMount } from 'svelte';
  import Icon from '../Icon.svelte';
  import { deviceId, home, toast } from '../../lib/store.svelte.js';
  import { currentSubscription, disablePush, enablePush, pushBlocker, pushSupport, testPush } from '../../lib/push.js';

  let support = $state(pushSupport());
  let subscribed = $state(false);
  let busy = $state(false);

  const blocker = $derived(pushBlocker(support));
  const me = $derived(home.targetId || deviceId());

  onMount(async () => {
    subscribed = Boolean(await currentSubscription().catch(() => null));
  });

  async function enable() {
    busy = true;
    try {
      await enablePush(me);
      subscribed = true;
      toast(tr("Notifications activées sur cet appareil"));
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      support = pushSupport();
      busy = false;
    }
  }

  async function disable() {
    busy = true;
    try {
      await disablePush(me);
      subscribed = false;
      toast(tr("Notifications désactivées sur cet appareil"));
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  async function test() {
    busy = true;
    try {
      await testPush(me);
      toast(tr("Essai envoyé : il doit arriver dans quelques secondes"));
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }
</script>

<section class="card" class:on={subscribed && !blocker}>
  <div class="head">
    <span class="ic"><Icon name="bell" size={22} /></span>
    <div>
      <strong>{tr("Sur ce téléphone")}</strong>
      <span>{subscribed && !blocker ? tr("Les alertes arrivent même Heim fermé") : tr("Recevoir les alertes même Heim fermé")}</span>
    </div>
  </div>

  {#if blocker === 'https'}
    <p>{tr("Il faut ouvrir Heim par son adresse sécurisée (qui commence par")} <code>https://</code>{tr("). Suivez")} <a href="#reglages/acces">{tr("Accès à distance")}</a> {tr(": c'est la même adresse à la maison et dehors.")}</p>
  {:else if blocker === 'install'}
    <ol>
      <li>{tr("Dans Safari, touchez")} <strong>{tr("Partager")}</strong> <span class="share" aria-hidden="true">⬆︎</span>.</li>
      <li>{tr("Choisissez")} <strong>{tr("Sur l'écran d'accueil")}</strong>{tr(", puis")} <strong>{tr("Ajouter")}</strong>.</li>
      <li>{tr("Ouvrez Heim depuis sa nouvelle icône et revenez ici.")}</li>
    </ol>
    <p class="muted">{tr("Apple n'autorise les notifications qu'aux applications ajoutées à l'écran d'accueil (iOS 16.4 ou plus récent).")}</p>
  {:else if blocker === 'unsupported'}
    <p>{tr("Ce navigateur ne sait pas recevoir de notifications. Sur iPhone, mettez iOS à jour (16.4 ou plus récent).")}</p>
  {:else if blocker === 'denied'}
    <p>{tr("Les notifications de Heim sont bloquées. Autorisez-les dans les Réglages du téléphone, rubrique Notifications, puis Heim.")}</p>
  {:else if subscribed}
    <div class="row">
      <button class="btn" disabled={busy} onclick={test}>{tr("Envoyer un essai")}</button>
      <button class="btn quiet" disabled={busy} onclick={disable}>{tr("Désactiver")}</button>
    </div>
  {:else}
    <button class="btn" disabled={busy} onclick={enable}>{tr("Activer les notifications")}</button>
  {/if}
</section>

<style>
  .card {
    padding: 16px;
    margin-bottom: 26px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .head {
    display: flex;
    gap: 12px;
    align-items: center;
    margin-bottom: 14px;
  }

  .head div {
    display: grid;
  }

  .head span:not(.ic) {
    font-size: 14px;
    color: var(--muted);
  }

  .ic {
    display: grid;
    place-items: center;
    flex: none;
    width: 44px;
    height: 44px;
    border-radius: 14px;
    background: var(--surface-2);
    color: var(--muted);
  }

  .on .ic {
    background: color-mix(in srgb, var(--moss) 22%, var(--surface));
    color: var(--moss);
  }

  p {
    margin: 0 0 8px;
  }

  ol {
    margin: 0 0 10px;
    padding-left: 22px;
    display: grid;
    gap: 6px;
  }

  .share {
    display: inline-block;
    padding: 0 4px;
    border-radius: 4px;
    background: var(--surface-2);
  }

  .muted {
    font-size: 14px;
    color: var(--muted);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }

  a {
    color: inherit;
    font-weight: 600;
  }
</style>
