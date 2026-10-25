<script>
  // Ajouter un appareil Zigbee sans passer par Zigbee2MQTT : on ouvre le réseau quelques
  // minutes et on suit l'arrivée de l'appareil en direct.
  import Sheet from './Sheet.svelte';
  import Icon from './Icon.svelte';
  import DeviceSheet from './DeviceSheet.svelte';
  import { api, home, loadPairing, toast } from '../lib/store.svelte.js';
  import { groupDevices } from '../lib/house.js';
  import { clock } from '../lib/format.js';

  let { onclose } = $props();
  const DURATION = 240;

  let busy = $state(false);
  let naming = $state(null);

  $effect(() => {
    loadPairing();
  });

  const p = $derived(home.pairing);
  const left = $derived(p.open && p.until ? Math.max(0, Math.round((Date.parse(p.until) - home.now) / 1000)) : null);
  const open = $derived(p.open && (left == null || left > 0));
  const devices = $derived(groupDevices(Object.values(home.entities)));

  async function start(seconds) {
    busy = true;
    try {
      await api('/zigbee/permit_join', { method: 'POST', body: { seconds } });
      if (seconds === 0) toast('Appairage arrêté');
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  function describe(ev) {
    switch (ev.event) {
      case 'device_joined':
        return { text: 'Nouvel appareil détecté', sub: 'Identification en cours, ne le débranchez pas.', icon: 'sensor' };
      case 'device_announce':
        return { text: "L'appareil s'annonce", sub: '', icon: 'sensor' };
      case 'device_interview':
        if (ev.detail?.startsWith('successful'))
          return { text: `${ev.friendly_name} est prêt`, sub: ev.detail.split(' : ')[1] ?? '', icon: 'check', ok: true };
        if (ev.detail === 'failed')
          return {
            text: "L'identification a échoué",
            sub: "Rapprochez l'appareil de la clé Zigbee, puis remettez-le en mode appairage.",
            icon: 'close',
            bad: true,
          };
        return { text: 'Identification…', sub: ev.friendly_name, icon: 'sensor' };
      case 'device_leave':
        return { text: `${ev.friendly_name} a quitté le réseau`, sub: '', icon: 'close' };
      default:
        return { text: ev.event.replace(/_/g, ' '), sub: ev.friendly_name, icon: 'sensor' };
    }
  }

  /** L'appareil arrivé, tel que Heim le voit maintenant (pour le nommer et le ranger). */
  function deviceOf(ev) {
    return devices.find((d) => d.entities.some((e) => e.capabilities?.device_name === ev.friendly_name)) ?? null;
  }
</script>

<Sheet title="Ajouter un appareil" {onclose}>
  {#if open}
    <div class="live" role="status">
      <span class="pulse" aria-hidden="true"></span>
      <div>
        <strong>Le réseau Zigbee accepte de nouveaux appareils</strong>
        <span>{left != null ? `Encore ${clock(left)}` : 'Ouvert'}</span>
      </div>
    </div>
    <ol class="how">
      <li>Mettez l'appareil en mode appairage : en général, maintenez son bouton 5 secondes jusqu'à ce que sa lumière clignote. Une ampoule : allumez-éteignez-la 5 fois.</li>
      <li>Restez près de la clé Zigbee le temps de l'identification (quelques secondes à une minute).</li>
    </ol>
    <button class="btn quiet" disabled={busy} onclick={() => start(0)}>Arrêter l'appairage</button>
  {:else}
    <p>Heim va ouvrir votre réseau Zigbee pendant 4 minutes. Pendant ce temps, n'importe quel appareil Zigbee en mode appairage peut le rejoindre.</p>
    <button class="btn" disabled={busy} onclick={() => start(DURATION)}><Icon name="plus" size={18} /> Lancer l'appairage</button>
    {#if p.version}<p class="muted">Zigbee2MQTT {p.version}</p>{/if}
  {/if}

  {#if p.events.length}
    <h3>Ce qui se passe</h3>
    <ul class="events">
      {#each p.events as ev (ev.at + ev.event + ev.friendly_name)}
        {@const d = describe(ev)}
        {@const dev = d.ok ? deviceOf(ev) : null}
        <li class:ok={d.ok} class:bad={d.bad}>
          <span class="ic"><Icon name={d.icon} size={18} /></span>
          <span class="txt"><strong>{d.text}</strong>{#if d.sub}<span>{d.sub}</span>{/if}</span>
          {#if dev}
            <button class="btn quiet small" onclick={() => (naming = dev)}>Nommer</button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</Sheet>

{#if naming}
  <DeviceSheet device={naming} onclose={() => (naming = null)} />
{/if}

<style>
  p {
    margin: 0 0 16px;
  }

  .muted {
    margin-top: 14px;
    font-size: 13px;
    color: var(--muted);
  }

  .live {
    display: flex;
    gap: 14px;
    align-items: center;
    padding: 14px 16px;
    border-radius: 16px;
    background: color-mix(in srgb, var(--moss) 16%, var(--surface));
  }

  .live div {
    display: grid;
  }

  .live span:not(.pulse) {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  .pulse {
    flex: none;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--moss);
    animation: pulse 1.6s ease-out infinite;
  }

  @keyframes pulse {
    0% {
      box-shadow: 0 0 0 0 color-mix(in srgb, var(--moss) 60%, transparent);
    }
    100% {
      box-shadow: 0 0 0 14px transparent;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .pulse {
      animation: none;
    }
  }

  .how {
    margin: 16px 0;
    padding-left: 20px;
    display: grid;
    gap: 8px;
  }

  h3 {
    font-size: 15px;
    font-weight: 600;
    color: var(--muted);
    margin: 26px 0 8px;
  }

  .events {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .events li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
  }

  .ic {
    display: grid;
    place-items: center;
    flex: none;
    width: 34px;
    height: 34px;
    border-radius: 50%;
    background: var(--surface-2);
  }

  .ok .ic {
    background: color-mix(in srgb, var(--moss) 25%, var(--surface));
    color: var(--moss);
  }

  .bad .ic {
    background: color-mix(in srgb, var(--alert) 18%, var(--surface));
    color: var(--alert);
  }

  .txt {
    display: grid;
    flex: 1;
    min-width: 0;
  }

  .txt span {
    font-size: 14px;
    color: var(--muted);
  }

  .small {
    padding: 6px 12px;
    min-height: 0;
  }
</style>
