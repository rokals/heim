<script>
  import { tr } from '../../lib/i18n.js';
  import { onMount } from 'svelte';
  import { api, home, toast } from '../../lib/store.svelte.js';
  import Icon from '../Icon.svelte';

  let a = $state(null);
  let c = $state(null);

  onMount(async () => {
    a = await api('/settings/alerts').catch(() => null);
    c = await api('/center/settings').catch(() => null);
  });

  // Les mesures qu'on peut surveiller : puissance, énergie, et toute autre valeur chiffrée.
  const measures = $derived(
    Object.values(home.entities)
      .filter((e) => e.domain === 'sensor' && typeof e.state?.value === 'number' && !e.hidden)
      .sort((x, y) => {
        const p = (e) => (['W', 'kW', 'kWh', 'Wh', 'VA', 'A'].includes(e.capabilities?.unit) ? 0 : 1);
        return p(x) - p(y) || x.name.localeCompare(y.name, 'fr');
      }),
  );
  const unit = (id) => home.entities[id]?.capabilities?.unit ?? '';

  async function saveCenter() {
    try {
      c = await api('/center/settings', { method: 'PUT', body: $state.snapshot(c) });
      toast(tr("Alertes enregistrées"));
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  function addRule() {
    const e = measures[0];
    c.energy.push({ id: '', name: '', entity_id: e?.entity_id ?? '', above: unit(e?.entity_id) === 'W' ? 2000 : 0, for_min: 5, push: true });
  }

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

{#if c}
  <h2><Icon name="sensor" size={20} /> {tr("La box")}</h2>
  <p class="muted">{tr("Une alerte apparaît dans la cloche au premier seuil ; au second, elle part aussi sur les téléphones des admins.")}</p>
  {#each [['memory', tr("Mémoire"), '%'], ['disk', tr("Carte SD"), '%'], ['temp', tr("Température du processeur"), '°C']] as [k, label, u] (k)}
    <div class="opt">
      <strong>{label}</strong>
      <label class="sub">
        {tr("alerte à")}
        <input class="input num" type="number" min="10" max="100" bind:value={c[`${k}_warn`]} onchange={saveCenter} />
        {u}, {tr("critique à")}
        <input class="input num" type="number" min="10" max="100" bind:value={c[`${k}_crit`]} onchange={saveCenter} />
        {u}
      </label>
    </div>
  {/each}
  <div class="opt">
    <label class="check"><input type="checkbox" bind:checked={c.push_critical} onchange={saveCenter} /> {tr("Prévenir les téléphones des admins quand c'est critique")}</label>
  </div>

  <h2><Icon name="bolt" size={20} /> {tr("Consommation")}</h2>
  <p class="muted">{tr("Être prévenu quand une mesure dépasse un seuil pendant un moment : la prise du chauffe-eau, le Linky, un congélateur qui force…")}</p>
  {#each c.energy as r, i (i)}
    <div class="rule">
      <div class="row">
        <select class="input grow" bind:value={r.entity_id} onchange={saveCenter}>
          {#each measures as e (e.entity_id)}<option value={e.entity_id}>{e.name}{e.room ? ` (${e.room})` : ''}{e.capabilities?.unit ? ` · ${e.capabilities.unit}` : ''}</option>{/each}
        </select>
        <button class="x" onclick={() => { c.energy.splice(i, 1); saveCenter(); }} aria-label={tr("Retirer")}><Icon name="close" size={16} /></button>
      </div>
      <label class="sub">
        {tr("au-dessus de")}
        <input class="input num" type="number" step="any" bind:value={r.above} onchange={saveCenter} />
        {unit(r.entity_id)} {tr("pendant")}
        <input class="input num" type="number" min="0" max="1440" bind:value={r.for_min} onchange={saveCenter} />
        {tr("min")}
      </label>
      <label class="sub">
        <input type="checkbox" bind:checked={r.push} onchange={saveCenter} />
        {tr("aussi sur les téléphones")}
      </label>
    </div>
  {/each}
  {#if measures.length}
    <button class="btn quiet" onclick={addRule}>{tr("+ Ajouter une alerte de consommation")}</button>
  {:else}
    <p class="muted">{tr("Aucune mesure chiffrée pour l'instant (prise connectée, Linky…).")}</p>
  {/if}
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

  h2 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 28px 0 4px;
    font-size: 19px;
  }

  .opt strong {
    display: block;
  }

  .sub {
    flex-wrap: wrap;
  }

  .rule {
    padding: 12px;
    margin-bottom: 10px;
    border-radius: 14px;
    background: var(--surface);
  }

  .rule .sub {
    margin-left: 0;
  }

  .row {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .x {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    color: var(--muted);
  }
</style>
