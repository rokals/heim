<script>
  import { tr } from '../../lib/i18n.js';
  // Zigbee : sur la box (Zigbee2MQTT installé ici, broker intégré) ou ailleurs (un
  // Zigbee2MQTT et son broker sur une autre machine, Home Assistant par exemple).
  import { onMount } from 'svelte';
  import ConnectionForm from '../ConnectionForm.svelte';
  import ZigbeeLocal from './ZigbeeLocal.svelte';
  import { api } from '../../lib/store.svelte.js';

  let mode = $state(null);

  onMount(async () => {
    const [local, remote] = await Promise.all([api('/zigbee/local').catch(() => null), api('/settings/zigbee2mqtt').catch(() => null)]);
    const elsewhere = remote?.host && !['127.0.0.1', 'localhost'].includes(remote.host.trim());
    mode = local?.enabled ? 'local' : elsewhere ? 'remote' : local?.installed ? 'local' : 'remote';
  });
</script>

{#if mode}
  <div class="tabs" role="tablist" aria-label={tr("Où tourne Zigbee2MQTT ?")}>
    <button role="tab" aria-selected={mode === 'local'} class:on={mode === 'local'} onclick={() => (mode = 'local')}>{tr("Sur la box")}</button>
    <button role="tab" aria-selected={mode === 'remote'} class:on={mode === 'remote'} onclick={() => (mode = 'remote')}>{tr("Sur une autre machine")}</button>
  </div>
  {#if mode === 'local'}
    <p class="muted">{tr("La clé Zigbee se branche sur la box : Zigbee2MQTT et le broker MQTT tournent ici, sans rien d'autre.")}</p>
    <ZigbeeLocal />
  {:else}
    <p class="muted">{tr("Heim lit vos appareils Zigbee par le broker MQTT de Zigbee2MQTT (souvent installé avec Home Assistant). Les deux peuvent tourner en même temps.")}</p>
    <ConnectionForm />
  {/if}
{/if}

<style>
  .tabs {
    display: flex;
    padding: 3px;
    border-radius: 14px;
    background: var(--surface-2);
    margin: 8px 0 12px;
  }

  .tabs button {
    flex: 1;
    min-height: 40px;
    border-radius: 11px;
    font-weight: 600;
    color: var(--muted);
    padding: 0 6px;
  }

  .tabs button.on {
    background: var(--surface);
    color: var(--ink);
    box-shadow: var(--shadow);
  }
</style>
