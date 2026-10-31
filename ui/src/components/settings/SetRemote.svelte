<script>
  import { tr } from '../../lib/i18n.js';
  import { copyText } from '../../lib/clipboard.js';
  // Accès à distance avec Tailscale : une adresse https://… valable à la maison comme dehors,
  // sans ouvrir de port sur la box. C'est aussi ce qui permet les notifications sur iPhone.
  import { onMount } from 'svelte';
  import Icon from '../Icon.svelte';
  import { api, home, setToken, toast } from '../../lib/store.svelte.js';

  let info = $state(null);
  let token = $state('');

  onMount(async () => {
    info = await api('/remote').catch(() => null);
  });

  const ts = $derived(info?.tailscale);
  const url = $derived(ts?.url ?? null);
  const here = location.origin;
  const onTailscale = $derived(location.hostname.endsWith('.ts.net'));
  const port = $derived(info?.port ?? 8090);

  async function copy(text) {
    try {
      await copyText(text);
      toast(tr("Copié"));
    } catch {
      toast(tr("Copie impossible : sélectionnez le texte"), 'alert');
    }
  }
</script>

{#if info}
  <section class="status" class:ok={onTailscale}>
    <span class="ic"><Icon name={onTailscale ? 'shieldcheck' : 'link'} size={22} /></span>
    <div>
      {#if onTailscale}
        <strong>{tr("Accès à distance en place")}</strong>
        <span>{tr("Cette adresse marche partout, et les notifications peuvent être activées.")}</span>
      {:else if url}
        <strong>{tr("Tailscale est prêt")}</strong>
        <span>{tr("Ouvrez plutôt")} <a href={url}>{url}</a> {tr(": même adresse à la maison et dehors.")}</span>
      {:else}
        <strong>{tr("Heim n'est joignable qu'à la maison")}</strong>
        <span>{tr("Adresse actuelle : {here}", { here })}</span>
      {/if}
    </div>
  </section>

  {#if home.me?.open}
    <p class="warn">{tr("Avant d'ouvrir Heim à distance, créez votre compte dans")} <a href="#reglages/comptes">{tr("Comptes")}</a> {tr(": sinon n'importe qui sur votre réseau Tailscale pourrait tout commander.")}</p>
  {/if}

  {#if !onTailscale}
    <h2>{tr("Mettre en place Tailscale")}</h2>
    <p class="muted">{tr("Gratuit pour une maison, aucun port à ouvrir sur la box. Chaque téléphone qui doit accéder à Heim a l'application Tailscale, connectée au même compte.")}</p>
    <ol class="steps">
      <li class:done={ts?.installed}>
        <strong>{tr("Installer Tailscale sur la machine de Heim")}</strong>
        {#if ts?.installed}
          <span class="tick">{tr("Installé{v}", { v: ts.running ? tr(" et connecté") : tr(", pas encore connecté") })}</span>
        {/if}
        <p>{tr("Sur le Raspberry Pi :")}</p>
        <button class="cmd" onclick={() => copy('curl -fsSL https://tailscale.com/install.sh | sh && sudo tailscale up')}>
          <code>curl -fsSL https://tailscale.com/install.sh | sh && sudo tailscale up</code><Icon name="copy" size={16} />
        </button>
        <p class="muted">{tr("Sur Windows : installez Tailscale depuis tailscale.com, puis connectez-vous.")}</p>
      </li>
      <li>
        <strong>{tr("Activer les adresses https")}</strong>
        <p>{tr("Dans la console Tailscale, page")} <em>{tr("DNS")}</em> {tr(": activez")} <em>{tr("MagicDNS")}</em> {tr("puis")} <em>{tr("HTTPS Certificates")}</em>.</p>
      </li>
      <li class:done={Boolean(url)}>
        <strong>{tr("Publier Heim sur votre réseau Tailscale")}</strong>
        <button class="cmd" onclick={() => copy(`sudo tailscale serve --bg ${port}`)}>
          <code>sudo tailscale serve --bg {port}</code><Icon name="copy" size={16} />
        </button>
        <p class="muted">{tr("Sur Windows, la même commande sans")} <code>sudo</code>{tr(", dans un terminal administrateur.")}</p>
        {#if url}<p>{tr("Adresse de votre Heim :")} <a href={url}>{url}</a></p>{/if}
      </li>
      <li>
        <strong>{tr("Sur l'iPhone")}</strong>
        <p>{tr("Installez l'application Tailscale (App Store) et connectez-vous avec le même compte. Ouvrez ensuite")} {#if url}<a href={url}>{url}</a>{:else}{tr("l'adresse en")} <code>https://…ts.net</code>{/if} {tr("dans Safari.")}</p>
      </li>
      <li>
        <strong>{tr("Ajouter Heim à l'écran d'accueil")}</strong>
        <p>{tr("Safari,")} <em>{tr("Partager")}</em>, <em>{tr("Sur l'écran d'accueil")}</em>{tr(". Ouvrez Heim depuis l'icône, puis Réglages, Notifications, « Activer les notifications ».")}</p>
      </li>
    </ol>
  {/if}
{/if}

<details class="token">
  <summary>{tr("Code d'accès technique (HEIM_TOKEN)")}</summary>
  <p class="muted">{tr("Seulement si Heim a été lancé avec HEIM_TOKEN, pour des scripts. Les personnes utilisent leur compte.")}</p>
  <form
    onsubmit={(e) => {
      e.preventDefault();
      setToken(token);
      toast(tr("Code enregistré"));
      token = '';
    }}
  >
    <input class="input" type="password" bind:value={token} autocomplete="off" aria-label={tr("Code d'accès")} />
    <button class="btn quiet" type="submit" disabled={!token}>{tr("Enregistrer")}</button>
  </form>
</details>

<style>
  .status {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    padding: 16px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .status div {
    display: grid;
    gap: 2px;
  }

  .status span:not(.ic) {
    color: var(--muted);
    font-size: 15px;
    overflow-wrap: anywhere;
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

  .ok .ic {
    background: color-mix(in srgb, var(--moss) 22%, var(--surface));
    color: var(--moss);
  }

  a {
    color: inherit;
    font-weight: 600;
  }

  .warn {
    margin: 16px 0 0;
    padding: 12px 14px;
    border-radius: 14px;
    background: color-mix(in srgb, var(--alert) 12%, var(--surface));
  }

  h2 {
    margin-top: 28px !important;
  }

  .steps {
    margin: 0;
    padding-left: 22px;
    display: grid;
    gap: 18px;
  }

  .steps p {
    margin: 4px 0;
  }

  .done > strong::after {
    content: ' ✓';
    color: var(--moss);
  }

  .tick {
    display: block;
    font-size: 14px;
    color: var(--moss);
  }

  .cmd {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    margin: 6px 0;
    padding: 10px 12px;
    border-radius: 12px;
    background: var(--surface-2);
    text-align: left;
  }

  .cmd code {
    flex: 1;
    font-size: 13px;
    overflow-wrap: anywhere;
  }

  .token {
    margin-top: 32px;
  }

  .token summary {
    cursor: pointer;
    color: var(--muted);
    font-weight: 600;
  }

  .token form {
    display: flex;
    gap: 10px;
    margin-top: 8px;
  }
</style>
