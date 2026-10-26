<script>
  import { tr } from '../../lib/i18n.js';
  import { onMount } from 'svelte';
  import { api, toast } from '../../lib/store.svelte.js';

  let a = $state(null);

  onMount(async () => {
    a = await api('/settings/alerts').catch(() => null);
  });

  async function save() {
    try {
      a = await api('/settings/alerts', { method: 'PUT', body: $state.snapshot(a) });
      toast(tr("Alertes enregistrées"));
    } catch (e) {
      toast(e.message, 'alert');
    }
  }
</script>

<p class="muted">{tr("Ces alertes marchent toutes seules et préviennent tous les appareils. Chaque alerte n'est envoyée qu'une fois, jusqu'à ce que le problème soit réglé.")}</p>

{#if a}
  <div class="opt">
    <label class="check"><input type="checkbox" bind:checked={a.battery} onchange={save} /> <strong>{tr("Pile faible")}</strong></label>
    {#if a.battery}
      <label class="sub">
        {tr("sous")}
        <input class="input num" type="number" min="1" max="50" bind:value={a.battery_threshold} onchange={save} />
        %
      </label>
    {/if}
  </div>
  <div class="opt">
    <label class="check"><input type="checkbox" bind:checked={a.silent} onchange={save} /> <strong>{tr("Appareil muet")}</strong></label>
    {#if a.silent}
      <label class="sub">
        {tr("sans nouvelles depuis")}
        <input class="input num" type="number" min="1" max="168" bind:value={a.silent_hours} onchange={save} />
        {tr("heures")}
      </label>
    {/if}
  </div>
  <div class="opt">
    <label class="check"><input type="checkbox" bind:checked={a.leak} onchange={save} /> <strong>{tr("Fuite d'eau")}</strong>{tr(", tout de suite")}</label>
  </div>
  <div class="opt">
    <label class="check">
      <input type="checkbox" checked={a.integrations !== false} onchange={(e) => ((a.integrations = e.currentTarget.checked), save())} />
      <strong>{tr("Zigbee2MQTT injoignable")}</strong> {tr("depuis 5 minutes (et quand il revient)")}
    </label>
  </div>
{/if}

<style>
  .opt {
    padding: 14px 0;
    border-bottom: 1px solid var(--line);
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

  .sub {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 10px 0 0 30px;
    color: var(--muted);
  }

  .sub .input {
    width: 84px;
  }
</style>
