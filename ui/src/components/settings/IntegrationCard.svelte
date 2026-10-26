<script>
  import { tr } from '../../lib/i18n.js';
  // La même carte pour chaque intégration : nom, état, résumé, interrupteur, et un clic
  // pour ouvrir ses réglages. Une nouvelle intégration n'a qu'à fournir ces champs.
  import Icon from '../Icon.svelte';

  let { item, onopen = null, ontoggle, busy = false } = $props();

  const STATUS = {
    connected: { text: tr("Connecté"), tone: 'ok' },
    starting: { text: tr("Connexion…"), tone: 'wait' },
    disconnected: { text: tr("Injoignable"), tone: 'bad' },
    failed: { text: tr("En échec"), tone: 'bad' },
    stopped: { text: tr("Désactivé"), tone: 'off' },
  };
  const st = $derived(
    !item.configured ? { text: tr("À configurer"), tone: 'off' } : !item.enabled ? { text: tr("Désactivé"), tone: 'off' } : (STATUS[item.status] ?? { text: tr("Démarrage…"), tone: 'wait' }),
  );
</script>

<article class="card" class:off={!item.enabled}>
  <button class="main" onclick={onopen} disabled={!onopen} aria-label={tr("Réglages de {name}", { name: item.name })}>
    <span class="ic"><Icon name={item.icon} size={24} /></span>
    <span class="txt">
      <span class="top">
        <strong>{item.name}</strong>
        <span class="badge {st.tone}"><i></i>{st.text}</span>
      </span>
      <span class="desc">{item.description}</span>
      <span class="meta">
        {#if item.enabled && item.configured}{tr("{devices} appareil(s){v}", { devices: item.devices, v: item.detail ? ` · ${item.detail}` : '' })}{:else if item.summary}{item.summary}{/if}
      </span>
    </span>
    {#if onopen}<span class="chev" aria-hidden="true">›</span>{/if}
  </button>
  <button
    class="switch"
    class:on={item.enabled}
    role="switch"
    aria-checked={item.enabled}
    aria-label="{item.enabled ? tr("Désactiver") : tr("Activer")} {item.name}"
    disabled={busy || (!item.configured && !item.enabled)}
    title={!item.configured ? tr("Configurez-la d’abord") : ''}
    onclick={() => ontoggle(!item.enabled)}
  ><span></span></button>
</article>

<style>
  .card {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 14px 6px 6px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .main {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 14px;
    min-width: 0;
    padding: 12px 8px;
    text-align: left;
    border-radius: calc(var(--radius-tile) - 4px);
  }

  .ic {
    display: grid;
    place-items: center;
    flex: none;
    width: 52px;
    height: 52px;
    border-radius: 16px;
    background: var(--surface-2);
    color: var(--ink);
  }

  .off .ic {
    color: var(--muted);
  }

  .txt {
    flex: 1;
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  .top {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .top strong {
    font-size: 18px;
  }

  .desc,
  .meta {
    font-size: 14px;
    color: var(--muted);
    overflow-wrap: anywhere;
  }

  .badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 2px 9px;
    border-radius: var(--radius-chip);
    background: var(--surface-2);
    font-size: 13px;
    font-weight: 600;
    color: var(--muted);
  }

  .badge i {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--line);
  }

  .badge.ok {
    color: var(--moss);
  }

  .badge.ok i {
    background: var(--moss);
  }

  .badge.bad {
    color: var(--alert);
  }

  .badge.bad i {
    background: var(--alert);
  }

  .badge.wait i {
    background: var(--ember);
  }

  .chev {
    font-size: 26px;
    color: var(--muted);
  }

  .switch {
    flex: none;
    position: relative;
    width: 52px;
    height: 32px;
    border-radius: 16px;
    background: var(--line);
    transition: background 0.15s;
  }

  .switch span {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.25);
    transition: transform 0.15s;
  }

  .switch.on {
    background: var(--moss);
  }

  .switch.on span {
    transform: translateX(20px);
  }

  .switch:disabled {
    opacity: 0.4;
  }
</style>
