<script>
  import { tr } from '../lib/i18n.js';
  // Tout un appareil : le commander, le régler (tous ses réglages, pas seulement allumé /
  // éteint), le ranger, et voir ce qu'il dit de lui.
  import { onMount, untrack } from 'svelte';
  import Sheet from './Sheet.svelte';
  import LightPanel from './LightPanel.svelte';
  import ControlField from './ControlField.svelte';
  import Icon from './Icon.svelte';
  import { formatValue, label } from '../lib/format.js';
  import { ago, lastSeen, signal } from '../lib/house.js';
  import { groupControls, isPowerOn, valueAt } from '../lib/controls.js';
  import { api, command, home, homeRooms, isAdmin, isMember, toast, updateDevice, currentBoard, saveBoard, boardRights } from '../lib/store.svelte.js';
  import SignalBars from './SignalBars.svelte';
  import ForSheet from './ForSheet.svelte';
  import HistoryChart from './HistoryChart.svelte';
  import JournalList from './JournalList.svelte';

  let { device, entity = null, onclose } = $props();

  const nameTarget = $derived(entity ?? device.primary);
  // Valeurs de départ du formulaire : on les copie une fois, puis l'utilisateur les modifie.
  const initialName = untrack(() => (entity ? entity.name : device.name));
  let name = $state(initialName);
  let room = $state(untrack(() => device.room));
  let saving = $state(false);
  let tab = $state('use');

  const rooms = $derived(homeRooms());
  const lqi = $derived(signal(device));
  const seen = $derived(lastSeen(device));
  const control = $derived(device.controls.find((c) => c.entity_id === nameTarget.entity_id) ?? device.controls[0]);
  const lightEntity = $derived(device.controls.find((c) => c.domain === 'light'));
  const onDashboard = $derived(
    (currentBoard()?.sections || []).some((sec) => sec.cards.some((c) => device.entities.some((e) => e.entity_id === c.entity_id))),
  );
  let forOpen = $state(false);
  // Un appareil qui ne répond plus du tout (retiré du réseau, reste d'essai) peut être supprimé.
  const dead = $derived(device.entities.every((e) => e.state?.available !== true));

  // ---- Réglages complets (Zigbee, Tuya) ----
  const deviceId = $derived(device.primary.device_id);
  let ctl = $state(null);
  const groups = $derived(groupControls(ctl?.controls ?? []));
  const settings = $derived([...groups.settings].sort((a, b) => Number(isPowerOn(b)) - Number(isPowerOn(a))));
  const update = $derived(ctl?.values?.update);
  const zigbee = $derived(ctl?.integration === 'zigbee2mqtt');

  async function loadControls() {
    if (!deviceId) return;
    try {
      ctl = await api(`/devices/${encodeURIComponent(deviceId)}/controls`);
    } catch {
      ctl = null;
    }
  }

  onMount(() => {
    loadControls();
    // Les réglages ne passent pas par le flux en direct : on relit de temps en temps.
    const t = setInterval(loadControls, 4000);
    return () => clearInterval(t);
  });

  async function setValue(key, value) {
    try {
      await api(`/devices/${encodeURIComponent(deviceId)}/set`, { method: 'POST', body: { values: { [key]: value } } });
      if (ctl) ctl.values = { ...ctl.values, [key]: value };
      setTimeout(loadControls, 1200);
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  async function action(name, force = false) {
    try {
      await api(`/devices/${encodeURIComponent(deviceId)}/action`, { method: 'POST', body: { action: name, force } });
      return true;
    } catch (e) {
      toast(e.message, 'alert');
      return false;
    }
  }

  async function removeFromZigbee() {
    if (!confirm(tr("Retirer « {name} » du réseau Zigbee ? Il faudra le réappairer pour l'utiliser.", { name }))) return;
    const force = dead && confirm(tr("L'appareil ne répond pas. Le retirer de force (sans attendre sa réponse) ?"));
    if (await action('remove', force)) {
      toast(tr("{name} retiré du réseau", { name }));
      setTimeout(removeDevice, 3000);
    }
  }

  let testing = $state(false);
  /** Trois secondes, pour vérifier qu'elle marche sans réveiller tout le quartier. */
  function testSiren() {
    const s = device.sirens[0];
    testing = true;
    command(s.entity_id, 'turn_on');
    setTimeout(() => {
      command(s.entity_id, 'turn_off');
      testing = false;
    }, 3000);
  }

  async function removeDevice() {
    try {
      for (const e of device.entities) await api(`/entities/${e.entity_id}`, { method: 'DELETE' }).catch(() => {});
      onclose();
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  async function forget() {
    if (!confirm(tr("Supprimer « {name} » de Heim ? S'il revient sur le réseau, il réapparaîtra dans « À ranger ».", { name }))) return;
    await removeDevice();
    toast(tr("{name} supprimé", { name }));
  }

  function addToHome() {
    const board = currentBoard();
    const sections = $state.snapshot(board.sections);
    if (!sections.length) sections.push({ title: tr("Accueil"), cards: [] });
    const wide = !device.controls.length;
    sections[sections.length - 1].cards.push({ entity_id: (control ?? device.primary).entity_id, ...(wide ? { w: 4, h: 1 } : { w: 2, h: 2 }) });
    saveBoard(board.id, sections);
    toast(tr("{name} ajouté à l'accueil", { name }));
  }

  async function save(extra = {}) {
    saving = true;
    try {
      await updateDevice(device, { name, room: room.trim(), ...extra }, nameTarget);
      toast(extra.hidden ? tr("{name} est masqué. Réglages pour le retrouver.", { name }) : tr("Modifications enregistrées"));
      onclose();
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      saving = false;
    }
  }

  const TABS = $derived(
    [
      { id: 'use', label: tr("Commandes") },
      (isAdmin() || settings.length) && { id: 'settings', label: tr("Réglages") },
      { id: 'history', label: tr("Historique") },
      { id: 'info', label: tr("Infos") },
    ].filter(Boolean),
  );

  // ---- Historique : courbes des mesures, journal de l'appareil ----
  const curves = $derived(
    device.readings.filter((e) => e.domain === 'sensor' && typeof e.state?.value === 'number'),
  );
  const logged = $derived(device.entities.filter((e) => e.category !== 'diagnostic').map((e) => e.entity_id));

  // ---- Pile ----
  const batteryEntity = $derived(device.entities.find((e) => e.capabilities?.kind === 'battery'));
  const waitingBattery = $derived(!!batteryEntity?.state?.attributes?.stale_since);

  async function batteryChanged() {
    try {
      await api(`/entities/${encodeURIComponent(batteryEntity.entity_id)}/battery_reset`, { method: 'POST' });
      toast(tr("Noté : le nouveau niveau s’affichera dès que l’appareil le donnera"));
      if (zigbee) action('read_battery');
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  const infoRows = $derived(
    [
      [tr("Fabricant"), ctl?.info?.vendor ?? device.primary.capabilities?.manufacturer],
      [tr("Modèle"), ctl?.info?.description ? `${ctl.info.description} (${ctl.info.model})` : (ctl?.info?.model ?? device.primary.capabilities?.model)],
      [tr("Alimentation"), ctl?.info?.power_source],
      [tr("Logiciel interne"), ctl?.info?.software_build_id ?? ctl?.info?.date_code],
      [tr("Protocole"), ctl?.info?.protocol ? tr("Tuya {protocol}", { protocol: ctl.info.protocol }) : null],
      [tr("Identifiant"), deviceId],
    ].filter(([, v]) => v),
  );
</script>

<Sheet title={initialName} {onclose}>
  <nav class="tabs" aria-label={tr("Sections de l'appareil")}>
    {#each TABS as t (t.id)}
      <button class:sel={tab === t.id} aria-pressed={tab === t.id} onclick={() => (tab = t.id)}>{t.label}</button>
    {/each}
  </nav>

  {#if tab === 'use'}
    {#if lightEntity}
      <LightPanel entity={lightEntity} light={groups.light} values={ctl?.values ?? {}} {deviceId} canSet={isMember() && Boolean(ctl)} />
    {/if}
    {#each device.controls.filter((c) => c !== lightEntity) as c (c.entity_id)}
      {@const on = c.state?.value === 'on'}
      <div class="switchrow">
        <span><Icon name="plug" size={18} /> {c.name}</span>
        <button class="power" class:on onclick={() => command(c.entity_id, on ? 'turn_off' : 'turn_on')} aria-pressed={on}>{on ? tr("Allumé") : tr("Éteint")}</button>
      </div>
    {/each}
    {#if !device.controls.length && !device.sirens?.length}
      <ul class="readings">
        {#each device.readings as e (e.entity_id)}
          <li><span>{label(e)}</span><strong class="num">{formatValue(e)}</strong></li>
        {/each}
      </ul>
    {/if}
    <div class="row">
      {#if control}
        <button class="btn quiet" type="button" onclick={() => (forOpen = true)}><Icon name="timer" size={18} /> {tr("Allumer pour…")}</button>
      {/if}
      {#if device.sirens?.length && isMember()}
        <button class="btn quiet" type="button" disabled={testing} onclick={testSiren}>{testing ? tr("Elle sonne…") : tr("Tester la sirène (3 s)")}</button>
      {/if}
    </div>
  {:else if tab === 'settings'}
    {#if isAdmin()}
      <form
        class="ident"
        onsubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <label class="field">
          <span>{tr("Nom")}</span>
          <input class="input" bind:value={name} autocomplete="off" />
        </label>
        <label class="field">
          <span>{tr("Pièce")}</span>
          <select class="input" bind:value={room}>
            <option value="">{tr("Sans pièce")}</option>
            {#each rooms as r (r)}<option value={r}>{r}</option>{/each}
            {#if room && !rooms.includes(room)}<option value={room}>{room}</option>{/if}
          </select>
        </label>
        <div class="row">
          <button class="btn" type="submit" disabled={saving}>{tr("Enregistrer")}</button>
          {#if device.hidden}
            <button class="btn quiet" type="button" disabled={saving} onclick={() => save({ hidden: false })}>{tr("Afficher à nouveau")}</button>
          {:else}
            <button class="btn quiet" type="button" disabled={saving} onclick={() => save({ hidden: true })}>{tr("Masquer")}</button>
          {/if}
          {#if currentBoard() && !currentBoard().auto && boardRights().add && !onDashboard && !device.hidden && !device.sirens?.length}
            <button class="btn quiet" type="button" onclick={addToHome}>{tr("Ajouter à l'accueil")}</button>
          {/if}
        </div>
        <p class="hint">{tr("Les pièces se créent dans Réglages,")} <a href="#reglages/maison" onclick={onclose}>{tr("Maison")}</a>.</p>
      </form>
    {/if}

    {#if settings.length}
      <h3>{tr("Réglages de l'appareil")}</h3>
      {#each groups.main as c (c.key)}
        <ControlField control={c} value={valueAt(ctl.values, c.key)} disabled={!isMember()} onchange={(v) => setValue(c.key, v)} />
      {/each}
      {#each settings as c (c.key)}
        <ControlField control={c} value={valueAt(ctl.values, c.key)} disabled={!isAdmin()} onchange={(v) => setValue(c.key, v)} />
      {/each}
    {:else if ctl}
      <p class="hint">{tr("Cet appareil n'a pas d'autre réglage.")}</p>
    {/if}
  {:else if tab === 'history'}
    {#each curves as e (e.entity_id)}
      <section class="curve">
        <h3>{label(e)} <span class="num now">{formatValue(e)}</span></h3>
        <HistoryChart entity={e.entity_id} unit={e.capabilities?.unit ? ` ${e.capabilities.unit}` : ''} />
      </section>
    {/each}
    {#if isMember()}
      <h3>{tr("Journal")}</h3>
      <JournalList entities={logged} limit={30} compact />
    {/if}
  {:else}
    <section class="health">
      <div>
        <span class="k">{tr("Signal")}</span>
        <span class="v">{#if lqi != null}<SignalBars {lqi} /> <span class="num">{lqi} / 255</span>{:else}{tr("inconnu")}{/if}</span>
      </div>
      <div>
        <span class="k">{tr("Dernier message")}</span>
        <span class="v">{ago(seen, home.now)}</span>
      </div>
      {#if batteryEntity}
        <div>
          <span class="k">{tr("Pile")}</span>
          <span class="v num" class:low={device.battery != null && device.battery <= 20}>
            {#if waitingBattery}{tr("en attente")}{:else if device.battery != null}{device.battery} %{:else}–{/if}
          </span>
        </div>
      {/if}
    </section>

    {#if batteryEntity}
      <section class="battery-box">
        <p>
          {#if waitingBattery}
            {tr("Pile changée {ago}. L'ancien niveau est ignoré : le nouveau s'affichera au prochain envoi de l'appareil (souvent au bout de quelques heures). Appuyez sur son bouton pour le réveiller plus vite.", { ago: ago(batteryEntity.state.attributes.stale_since, home.now) })}
          {:else if batteryEntity.state?.last_changed}
            {tr("Niveau changé pour la dernière fois {ago}. Un appareil sur pile ne l'envoie que de temps en temps.", { ago: ago(batteryEntity.state.last_changed, home.now) })}
          {/if}
        </p>
        {#if isMember()}
          <div class="row">
            <button class="btn quiet" onclick={batteryChanged}><Icon name="battery" size={17} /> {tr("Pile changée")}</button>
            {#if zigbee && isAdmin()}
              <button class="btn quiet" onclick={() => action('read_battery') && toast(tr("Demande envoyée : appuyez sur le bouton de l’appareil pour le réveiller"))}>{tr("Relire le niveau")}</button>
            {/if}
          </div>
        {/if}
      </section>
    {/if}

    {#if update && zigbee}
      <section class="ota" class:avail={update.state === 'available'}>
        {#if update.state === 'available'}
          <strong>{tr("Mise à jour du logiciel interne disponible")}</strong>
          <span>{update.installed_version ?? '?'} → {update.latest_version ?? '?'}</span>
          {#if isAdmin()}<button class="btn" onclick={() => action('ota_update') && toast(tr("Mise à jour lancée : elle peut prendre une heure, sans débrancher"))}>{tr("Mettre à jour")}</button>{/if}
        {:else if update.state === 'updating'}
          <strong>{tr("Mise à jour en cours… {v}", { v: update.progress != null ? `${Math.round(update.progress)} %` : '' })}</strong>
        {:else}
          <span>{tr("Logiciel interne à jour")}</span>
        {/if}
      </section>
    {/if}

    <h3>{tr("Ce que l'appareil indique")}</h3>
    <ul class="readings">
      {#each device.entities as e (e.entity_id)}
        <li>
          {#if e.domain === 'siren'}
            <span>{tr("Sirène")}</span>
            <strong>{e.state?.value === 'on' ? tr("Sonne") : tr("Silencieuse")}</strong>
          {:else}
            <span>{e.domain === 'light' || e.domain === 'switch' ? e.name : label(e)}</span>
            <strong class="num">{e.domain === 'light' || e.domain === 'switch' ? (e.state?.value === 'on' ? tr("Allumé") : tr("Éteint")) : formatValue(e)}</strong>
          {/if}
        </li>
      {/each}
    </ul>

    {#if infoRows.length}
      <dl class="info">
        {#each infoRows as [k, v] (k)}<dt>{k}</dt><dd>{v}</dd>{/each}
      </dl>
    {/if}

    {#if isAdmin()}
      <div class="row danger-zone">
        {#if zigbee}
          <button class="btn quiet" onclick={() => action('ota_check') && toast(tr("Recherche de mise à jour envoyée"))}>{tr("Chercher une mise à jour")}</button>
          <button class="btn danger" onclick={removeFromZigbee}>{tr("Retirer du réseau Zigbee")}</button>
        {/if}
        {#if dead}
          <button class="btn danger" onclick={forget}>{tr("Supprimer de Heim")}</button>
        {/if}
      </div>
    {/if}
  {/if}
</Sheet>

{#if forOpen && control}
  <ForSheet entity={control} onclose={() => ((forOpen = false), onclose())} />
{/if}

<style>
  .tabs {
    display: flex;
    padding: 3px;
    margin: 0 0 18px;
    border-radius: 14px;
    background: var(--surface-2);
  }

  .tabs button {
    flex: 1;
    min-height: 40px;
    border-radius: 11px;
    font-weight: 600;
    color: var(--muted);
  }

  .tabs button.sel {
    background: var(--surface);
    color: var(--ink);
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.08);
  }

  .row {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
    margin-top: 18px;
  }

  .switchrow {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 0;
    border-bottom: 1px solid var(--line);
  }

  .switchrow span {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-weight: 600;
  }

  .power {
    min-width: 92px;
    min-height: 40px;
    border-radius: var(--radius-chip);
    background: var(--surface-2);
    font-weight: 600;
  }

  .power.on {
    background: var(--ember);
    color: var(--ember-ink);
  }

  .ident {
    margin-bottom: 10px;
  }

  .hint {
    font-size: 14px;
    color: var(--muted);
  }

  .hint a {
    color: inherit;
    font-weight: 600;
  }

  h3 {
    font-size: 15px;
    font-weight: 600;
    color: var(--muted);
    margin: 26px 0 4px;
  }

  .health {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: 12px;
    padding: 14px 16px;
    border-radius: 16px;
    background: var(--surface);
  }

  .health div {
    display: grid;
    gap: 2px;
  }

  .k {
    font-size: 13px;
    color: var(--muted);
  }

  .v {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-weight: 600;
  }

  .low {
    color: var(--alert);
  }

  .ota {
    display: grid;
    gap: 6px;
    margin-top: 14px;
    padding: 14px 16px;
    border-radius: 16px;
    background: var(--surface);
    color: var(--muted);
  }

  .ota.avail {
    box-shadow: inset 0 0 0 2px var(--moss);
    color: var(--ink);
  }

  .ota .btn {
    justify-self: start;
  }

  .battery-box {
    padding: 12px 14px;
    border-radius: 16px;
    background: var(--surface-2);
    margin: 10px 0;
  }

  .battery-box p {
    margin: 0 0 8px;
    font-size: 14px;
    color: var(--muted);
  }

  .curve h3 {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }

  .curve .now {
    font-size: 15px;
    font-weight: 600;
    color: var(--muted);
  }

  .readings {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .readings li {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    padding: 11px 0;
    border-bottom: 1px solid var(--line);
  }

  .readings strong {
    text-align: right;
  }

  .info {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 6px 14px;
    margin: 18px 0 0;
    font-size: 14px;
  }

  .info dt {
    color: var(--muted);
  }

  .info dd {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .danger-zone {
    margin-top: 26px;
  }
</style>
