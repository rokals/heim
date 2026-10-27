<script>
  import { tr } from '../../lib/i18n.js';
  // Toutes les intégrations au même endroit, puis la page de chacune.
  import { onMount } from 'svelte';
  import IntegrationCard from './IntegrationCard.svelte';
  import ZigbeeSetup from './ZigbeeSetup.svelte';
  import SetBroker from './SetBroker.svelte';
  import PluginStore from './PluginStore.svelte';
  import PluginDetail from './PluginDetail.svelte';
  import PairingSheet from '../PairingSheet.svelte';
  import SetTuya from './SetTuya.svelte';
  import SetMatter from './SetMatter.svelte';
  import Icon from '../Icon.svelte';
  import { api, home, toast } from '../../lib/store.svelte.js';

  let { detail = '', onnavigate } = $props();

  let list = $state([]);
  let busy = $state(null);
  let pairing = $state(false);
  let power = $state(null);
  let applying = $state(false);
  let view = $state('installed');
  let removing = $state(false);

  async function uninstallBuiltin(id) {
    if (!removing) {
      removing = true;
      return;
    }
    try {
      await api(`/plugins/${id}`, { method: 'DELETE' });
      toast(tr("Connexion retirée : elle reste dans le store"));
      removing = false;
      onnavigate('integrations');
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  async function load() {
    list = await api('/integrations/catalog').catch(() => []);
  }

  async function loadPower() {
    power = await api('/power_on').catch(() => null);
  }

  onMount(() => {
    load();
    loadPower();
  });

  // L'état change en direct (connexion, nombre d'appareils) : on suit.
  $effect(() => {
    void JSON.stringify(home.integrations);
    load();
  });

  async function toggle(item, enabled) {
    busy = item.id;
    try {
      await api(`/integrations/${item.id}/enabled`, { method: 'PUT', body: { enabled } });
      toast(`${item.name} ${enabled ? tr("activé") : tr("désactivé")}`);
      await load();
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = null;
    }
  }

  async function savePower(patch) {
    applying = true;
    try {
      const r = await api('/power_on', { method: 'PUT', body: { ...$state.snapshot(power.settings), ...patch } });
      if (patch.behavior) {
        toast(r.applied ? tr("Réglé sur {applied} appareil(s)", { applied: r.applied }) : tr("Aucun appareil ne propose ce réglage"));
      }
      await loadPower();
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      applying = false;
    }
  }

  const current = $derived(list.find((i) => i.id === detail));
  const supported = $derived(power?.devices.filter((d) => d.supported.length) ?? []);
  const BEHAVIORS = [
    { id: 'previous', label: tr("Comme avant"), hint: tr("L’appareil reprend l’état qu’il avait") },
    { id: 'off', label: tr("Éteint"), hint: tr("Tout reste éteint au retour du courant") },
    { id: 'on', label: tr("Allumé"), hint: tr("Tout se rallume (pratique pour une lampe sans interrupteur)") },
  ];
</script>

{#if !detail}
  <div class="tabs" role="tablist">
    <button role="tab" aria-selected={view === 'installed'} class:on={view === 'installed'} onclick={() => (view = 'installed')}>{tr("Installées")}</button>
    <button role="tab" aria-selected={view === 'store'} class:on={view === 'store'} onclick={() => (view = 'store')}><Icon name="plus" size={16} /> {tr("Store")}</button>
  </div>
  {#if view === 'store'}
    <PluginStore oninstalled={load} />
  {:else}
  <p class="muted">{tr("Les passerelles vers vos appareils. Touchez-en une pour la régler, ou ajoutez-en depuis le store.")}</p>
  <div class="list">
    {#each list as item (item.id)}
      <IntegrationCard {item} busy={busy === item.id} onopen={() => onnavigate(`integrations/${item.id}`)} ontoggle={(on) => toggle(item, on)} />
    {/each}
  </div>

  {#if power}
    <section class="power">
      <h2><Icon name="power" size={20} /> {tr("Après une coupure de courant")}</h2>
      <p class="muted">{tr("Pour tous les appareils qui le permettent ({length} aujourd'hui), au lieu de les régler un par un.", { length: supported.length })}</p>
      <div class="choices" role="radiogroup" aria-label={tr("Après une coupure de courant")}>
        {#each BEHAVIORS as b (b.id)}
          <button role="radio" aria-checked={power.settings.behavior === b.id} class:sel={power.settings.behavior === b.id} disabled={applying} onclick={() => savePower({ behavior: b.id })}>
            <strong>{b.label}</strong>
            <span>{b.hint}</span>
          </button>
        {/each}
      </div>
      <label class="check">
        <input type="checkbox" checked={power.settings.apply_new} disabled={!power.settings.behavior} onchange={(e) => savePower({ apply_new: e.currentTarget.checked })} />
        {tr("Régler aussi les nouveaux appareils Zigbee dès leur ajout")}
      </label>
      {#if supported.length}
        <details>
          <summary>{tr("Voir les appareils concernés")}</summary>
          <ul class="rows">
            {#each supported as d (d.device_id)}
              <li><span>{d.name}</span><span class="muted">{d.integration === 'tuya' ? tr("Tuya") : tr("Zigbee")}{d.current != null ? tr("· actuellement : {current}", { current: d.current }) : ''}</span></li>
            {/each}
          </ul>
        </details>
      {/if}
    </section>
  {/if}
  {/if}
{:else if current}
  <div class="detail-head">
    <IntegrationCard item={current} busy={busy === current.id} ontoggle={(on) => toggle(current, on)} />
  </div>
  {#if current.id === 'zigbee2mqtt'}
    <h2>{tr("Où tourne Zigbee2MQTT ?")}</h2>
    <ZigbeeSetup />
    {#if current.status === 'connected'}
      <h2>{tr("Nouvel appareil")}</h2>
      <button class="btn" onclick={() => (pairing = true)}><Icon name="plus" size={18} /> {tr("Ajouter un appareil Zigbee")}</button>
    {/if}
  {:else if current.id === 'tuya'}
    <SetTuya embedded />
  {:else if current.id === 'matter'}
    <SetMatter embedded />
  {:else if current.id === 'mqtt'}
    <SetBroker />
  {:else if current.id === 'mqtt_discovery'}
    <p class="muted">{tr("Heim lit les appareils qui s'annoncent sur le broker au format de Home Assistant : Tasmota (commande SetOption19 1), ESPHome (mqtt: discovery: true), Shelly, OpenMQTTGateway… Ils apparaissent tout seuls dans Appareils.")}</p>
  {:else if current.id.startsWith('plugin:')}
    <PluginDetail id={current.id.slice(7)} onremoved={() => onnavigate('integrations')} />
  {/if}
  {#if ['zigbee2mqtt', 'tuya', 'matter', 'mqtt_discovery'].includes(current.id)}
    <p class="remove">
      <button class="link danger" onclick={() => uninstallBuiltin(current.id)}>{removing ? tr("Confirmer : retirer {v} des connexions", { v: current.name }) : tr("Retirer cette connexion")}</button>
    </p>
  {/if}
{/if}

{#if pairing}
  <PairingSheet onclose={() => (pairing = false)} />
{/if}

<style>
  .tabs {
    display: flex;
    padding: 3px;
    border-radius: 14px;
    background: var(--surface-2);
    margin-bottom: 14px;
  }

  .tabs button {
    flex: 1;
    min-height: 40px;
    border-radius: 11px;
    font-weight: 600;
    color: var(--muted);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
  }

  .tabs button.on {
    background: var(--surface);
    color: var(--ink);
    box-shadow: var(--shadow);
  }

  .remove {
    margin-top: 32px;
  }

  .danger {
    color: var(--alert);
  }

  .list {
    display: grid;
    gap: 12px;
  }

  .detail-head {
    margin-bottom: 8px;
  }

  h2 {
    margin-top: 28px !important;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .power {
    margin-top: 12px;
  }

  .choices {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: 10px;
    margin: 12px 0;
  }

  .choices button {
    display: grid;
    gap: 4px;
    padding: 14px;
    border-radius: 16px;
    background: var(--surface);
    box-shadow: var(--shadow);
    text-align: left;
    border: 2px solid transparent;
  }

  .choices button.sel {
    border-color: var(--ink);
  }

  .choices span {
    font-size: 13px;
    color: var(--muted);
  }

  .check {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .check input {
    width: 20px;
    height: 20px;
    accent-color: var(--ink);
  }

  details {
    margin-top: 12px;
  }

  summary {
    cursor: pointer;
    color: var(--muted);
    font-weight: 600;
  }

  .rows {
    list-style: none;
    padding: 0;
    margin: 8px 0 0;
  }

  .rows li {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 9px 0;
    border-bottom: 1px solid var(--line);
    font-size: 15px;
  }
</style>
