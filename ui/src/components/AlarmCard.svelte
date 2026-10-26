<script>
  import { tr } from '../lib/i18n.js';
  // La carte d'alarme, en haut de l'accueil : un coup d'œil pour savoir si la maison est
  // protégée, un geste pour l'activer ou l'arrêter.
  import Icon from './Icon.svelte';
  import Sheet from './Sheet.svelte';
  import CodeSheet from './CodeSheet.svelte';
  import { api, home, isAdmin, isMember, loadSecurity, snapshotUrl, toast } from '../lib/store.svelte.js';
  import { describe, MODES } from '../lib/security.js';

  const sec = $derived(home.security);
  const status = $derived(sec?.status);
  const config = $derived(sec?.config);
  const view = $derived(describe(status, home.now));

  /** Photos prises depuis le déclenchement. */
  const photos = $derived(
    view.tone === 'alarm' && sec
      ? sec.log.filter((l) => l.snapshot && Date.parse(l.at) >= Date.parse(status.since) - 5000).slice(0, 3)
      : [],
  );

  let ask = $state(null); // { kind: 'arm' | 'disarm', mode, force }
  let codeError = $state('');
  let notReady = $state(null);
  let busy = $state(false);

  async function arm(mode, force = false, code = undefined) {
    busy = true;
    try {
      await api('/security/arm', { method: 'POST', body: { mode, force, code } });
      ask = null;
      notReady = null;
      loadSecurity();
    } catch (e) {
      if (e.status === 409) {
        ask = null;
        notReady = { mode, code, open: e.data?.open ?? [], unavailable: e.data?.unavailable ?? [] };
      } else if (e.status === 403) {
        ask = { kind: 'arm', mode, force };
        codeError = e.message;
      } else toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  async function disarm(code = undefined) {
    busy = true;
    try {
      await api('/security/disarm', { method: 'POST', body: { code } });
      ask = null;
      loadSecurity();
    } catch (e) {
      if (e.status === 403) {
        ask = { kind: 'disarm' };
        codeError = e.message;
      } else toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  function requestArm(mode) {
    codeError = '';
    if (config?.has_code && config?.code_to_arm) ask = { kind: 'arm', mode, force: false };
    else arm(mode);
  }

  function requestDisarm() {
    codeError = '';
    if (config?.has_code) ask = { kind: 'disarm' };
    else disarm();
  }

  const stopLabel = $derived(
    status?.phase === 'arming' ? tr("Annuler") : view.tone === 'alarm' ? (view.h24 ? tr("Arrêter") : tr("Arrêter l'alarme")) : tr("Désactiver"),
  );
</script>

{#if status}
  <section class="alarm tone-{view.tone}" aria-live="polite">
    <div class="head">
      <span class="shield"><Icon name={view.tone === 'armed' ? 'shieldcheck' : view.tone === 'alarm' ? 'siren' : 'shield'} size={24} /></span>
      <div class="txt">
        <strong>{view.title}</strong>
        {#if view.line}<span class:num={view.left != null}>{view.line}</span>{/if}
      </div>
      {#if isAdmin()}<a class="cog" href="#reglages/securite" aria-label={tr("Réglages de l'alarme")}><Icon name="sliders" size={18} /></a>{/if}
    </div>

    {#if view.left != null}
      {@const total = status.phase === 'pending' ? config?.entry_delay_s : status.mode === 'night' ? config?.night_exit_delay_s : config?.exit_delay_s}
      <div class="meter" aria-hidden="true"><span style:width="{total ? (100 * view.left) / total : 0}%"></span></div>
    {/if}

    {#if photos.length}
      <div class="photos">
        {#each photos as p (p.snapshot)}
          <a href={snapshotUrl(p.snapshot)} target="_blank" rel="noopener"><img src={snapshotUrl(p.snapshot)} alt={p.message} /></a>
        {/each}
      </div>
    {/if}

    {#if isMember()}
    <div class="actions">
      {#if view.tone === 'off'}
        {#each Object.entries(MODES) as [id, m] (id)}
          <button class="btn mode" disabled={busy} onclick={() => requestArm(id)}>
            <Icon name={m.icon} size={18} /> {m.label}
          </button>
        {/each}
      {:else}
        <button class="btn stop" disabled={busy} onclick={requestDisarm}>{stopLabel}</button>
      {/if}
    </div>
    {/if}
  </section>
{/if}

{#if ask}
  <CodeSheet
    title={ask.kind === 'arm' ? tr("Activer : {label}", { label: MODES[ask.mode].label }) : view.tone === 'alarm' ? tr("Arrêter l'alarme") : tr("Désactiver l'alarme")}
    error={codeError}
    onsubmit={(code) => (ask.kind === 'arm' ? arm(ask.mode, ask.force, code) : disarm(code))}
    onclose={() => ((ask = null), (codeError = ''))}
  />
{/if}

{#if notReady}
  <Sheet title={tr("Tout n'est pas fermé")} onclose={() => (notReady = null)}>
    {#if notReady.open.length}
      <p>{tr("Encore ouvert :")}</p>
      <ul>{#each notReady.open as s (s.entity_id)}<li><Icon name="door" size={18} /> {s.name}</li>{/each}</ul>
    {/if}
    {#if notReady.unavailable.length}
      <p>{tr("Ne répond pas :")}</p>
      <ul>{#each notReady.unavailable as s (s.entity_id)}<li><Icon name="sensor" size={18} /> {s.name}</li>{/each}</ul>
    {/if}
    <p class="muted">{tr("En activant quand même, ces capteurs seront ignorés jusqu'à la prochaine désactivation.")}</p>
    <div class="row">
      <button class="btn" disabled={busy} onclick={() => arm(notReady.mode, true, notReady.code)}>{tr("Activer quand même")}</button>
      <button class="btn quiet" onclick={() => (notReady = null)}>{tr("Annuler")}</button>
    </div>
  </Sheet>
{/if}

<style>
  .alarm {
    margin: 0 0 22px;
    padding: 14px 16px 16px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .shield {
    flex: none;
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: 14px;
    background: var(--surface-2);
    color: var(--muted);
  }

  .txt {
    flex: 1;
    display: grid;
    min-width: 0;
  }

  .txt strong {
    font-size: 18px;
  }

  .txt span {
    color: var(--muted);
  }

  .cog {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    color: var(--muted);
  }

  .actions {
    display: flex;
    gap: 10px;
    margin-top: 14px;
  }

  .mode {
    flex: 1;
    justify-content: center;
    background: var(--surface-2);
    color: var(--ink);
  }

  .stop {
    flex: 1;
    justify-content: center;
  }

  /* Protégée : calme, la couleur de la mousse. */
  .tone-armed .shield {
    background: color-mix(in srgb, var(--moss) 22%, var(--surface));
    color: var(--moss);
  }

  .tone-arming .shield,
  .tone-pending .shield {
    color: var(--alert);
  }

  .tone-pending {
    box-shadow: inset 0 0 0 2px var(--alert);
  }

  .meter {
    height: 6px;
    margin-top: 12px;
    border-radius: 3px;
    background: var(--surface-2);
    overflow: hidden;
  }

  .meter span {
    display: block;
    height: 100%;
    background: var(--alert);
    transition: width 0.5s linear;
  }

  .tone-arming .meter span {
    background: var(--muted);
  }

  /* Alarme : impossible à manquer. */
  .tone-alarm {
    background: var(--alert);
    color: #fff;
  }

  .tone-alarm .txt span,
  .tone-alarm .cog {
    color: color-mix(in srgb, #fff 80%, transparent);
  }

  .tone-alarm .shield {
    background: color-mix(in srgb, #fff 18%, transparent);
    color: #fff;
    animation: blink 1s steps(2) infinite;
  }

  .tone-alarm .stop {
    background: #fff;
    color: var(--alert);
  }

  @keyframes blink {
    50% {
      opacity: 0.35;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .tone-alarm .shield {
      animation: none;
    }
  }

  .photos {
    display: flex;
    gap: 8px;
    margin-top: 12px;
    overflow-x: auto;
  }

  .photos img {
    display: block;
    height: 96px;
    border-radius: 10px;
  }

  ul {
    list-style: none;
    padding: 0;
    margin: 0 0 14px;
  }

  li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 0;
    font-weight: 600;
  }

  p {
    margin: 0 0 6px;
  }

  .muted {
    color: var(--muted);
    margin: 8px 0 18px;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }
</style>
