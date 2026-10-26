<script>
  import { tr } from '../../lib/i18n.js';
  // Tuya en local : importer le fichier de tinytuya, ou ajouter un appareil à la main.
  import { onMount } from 'svelte';
  import Icon from '../Icon.svelte';
  import { api, home, toast } from '../../lib/store.svelte.js';

  let { embedded = false } = $props();

  let cfg = $state(null);
  let found = $state(null);
  let scanning = $state(false);
  let editing = $state(null);
  let adding = $state(false);
  let blank = () => ({ id: '', name: '', key: '', ip: '', version: '', category: 'cz' });
  let draft = $state(blank());

  async function load() {
    cfg = await api('/tuya').catch(() => null);
  }

  onMount(load);

  const status = $derived(home.integrations.tuya);

  function deviceState(d) {
    const e = d.entities.map((id) => home.entities[id]).find(Boolean);
    if (!d.supported) return { text: tr("non pris en charge en local (caméra, sonnette ou appareil sur pile)"), bad: true };
    if (!e?.state) return { text: d.ip ? 'connexion…' : tr("en attente de son annonce réseau (ou indiquez l'adresse IP)") };
    return e.state.available ? { text: tr("connecté"), ok: true } : { text: 'injoignable', bad: true };
  }

  async function save(devices, enabled = cfg.enabled) {
    try {
      cfg = await api('/tuya', {
        method: 'PUT',
        body: {
          enabled,
          devices: devices.map((d) => ({ id: d.id, name: d.name, key: d.key || null, ip: d.ip || null, version: d.version || null, category: d.category || null })),
        },
      });
      toast(tr("Tuya enregistré"));
      return true;
    } catch (e) {
      toast(e.message, 'alert');
      return false;
    }
  }

  async function importFile(e) {
    const input = e.currentTarget;
    const file = input.files?.[0];
    if (!file) return;
    const text = await file.text();
    try {
      const r = await fetch('/api/tuya/import', { method: 'POST', body: text, headers: { 'content-type': 'text/plain' } });
      const data = await r.json();
      if (!r.ok) throw new Error(data.error);
      cfg = data;
      toast(data.added ? tr("{added} appareil(s) ajouté(s)", { added: data.added }) : tr("Appareils mis à jour"));
    } catch (err) {
      toast(err.message, 'alert');
    }
    input.value = '';
  }

  async function scan() {
    scanning = true;
    found = await api('/tuya/scan', { method: 'POST' }).catch(() => []);
    scanning = false;
  }

  async function saveEdit(e) {
    e.preventDefault();
    const list = cfg.devices.map((d) => (d.id === editing.id ? editing : d));
    if (await save(list)) editing = null;
  }

  async function addManual(e) {
    e.preventDefault();
    if (await save([...cfg.devices, draft], true)) {
      adding = false;
      draft = blank();
    }
  }

  async function remove(d) {
    if (!confirm(tr("Retirer « {name} » de Heim ?", { name: d.name }))) return;
    await save(cfg.devices.filter((x) => x.id !== d.id));
  }

  const known = (id) => cfg?.devices.some((d) => d.id === id);
</script>

<p class="muted">{tr("Prises, ampoules et interrupteurs Wi-Fi Tuya (LSC Action, Lidl, Silvercrest…), pilotés directement sur le réseau, sans le cloud. Il faut une fois la « clé locale » de chaque appareil.")}</p>

{#if cfg}
  {#if !embedded}
    <label class="check big">
      <input type="checkbox" checked={cfg.enabled} onchange={(e) => save(cfg.devices, e.currentTarget.checked)} />
      <strong>{tr("Activer Tuya")}</strong>
      {#if status?.detail}<span class="muted">— {status.detail}</span>{/if}
    </label>
  {/if}

  <h2>{tr("Appareils")}</h2>
  {#if !cfg.devices.length}
    <p class="muted">{tr("Aucun appareil pour l'instant.")}</p>
  {:else}
    <ul class="rows">
      {#each cfg.devices as d (d.id)}
        {@const s = deviceState(d)}
        <li>
          <span class="dot" class:ok={s.ok} class:bad={s.bad}></span>
          <div class="txt">
            <strong>{d.name}</strong>
            <span>{d.product_name ?? d.category ?? ''}{d.ip ? ` · ${d.ip}` : ''}{d.version ? ` · v${d.version}` : ''} · {s.text}</span>
          </div>
          <button class="mini" aria-label={tr("Modifier {name}", { name: d.name })} onclick={() => (editing = { ...d, key: '' })}><Icon name="pencil" size={16} /></button>
          <button class="mini" aria-label={tr("Retirer {name}", { name: d.name })} onclick={() => remove(d)}><Icon name="close" size={16} /></button>
        </li>
        {#if editing?.id === d.id}
          <li class="edit">
            <form onsubmit={saveEdit}>
              <input class="input" bind:value={editing.name} placeholder={tr("Nom")} />
              <input class="input" bind:value={editing.ip} placeholder={tr("Adresse IP (vide : trouvée toute seule)")} inputmode="decimal" />
              <select class="input" bind:value={editing.version}>
                <option value="">{tr("Version : automatique")}</option>
                <option value="3.3">3.3</option><option value="3.4">3.4</option><option value="3.5">3.5</option>
              </select>
              <input class="input" bind:value={editing.key} placeholder={tr("Nouvelle clé locale (vide : garder l'actuelle)")} autocomplete="off" />
              <div class="row">
                <button class="btn" type="submit">{tr("Enregistrer")}</button>
                <button class="btn quiet" type="button" onclick={() => (editing = null)}>{tr("Annuler")}</button>
              </div>
            </form>
          </li>
        {/if}
      {/each}
    </ul>
  {/if}

  <div class="row">
    <label class="btn">
      <Icon name="plus" size={18} /> {tr("Importer devices.json")}
      <input type="file" accept=".json,application/json" onchange={importFile} hidden />
    </label>
    <button class="btn quiet" onclick={() => (adding = !adding)}>{tr("Ajouter à la main")}</button>
    <button class="btn quiet" disabled={scanning} onclick={scan}>{scanning ? tr("Écoute du réseau…") : tr("Chercher sur le réseau")}</button>
  </div>

  {#if adding}
    <form class="manual" onsubmit={addManual}>
      <input class="input" bind:value={draft.name} placeholder={tr("Nom (Prise salon)")} />
      <input class="input" bind:value={draft.id} placeholder={tr("Identifiant (bf…)")} autocomplete="off" />
      <input class="input" bind:value={draft.key} placeholder={tr("Clé locale (16 caractères)")} autocomplete="off" />
      <input class="input" bind:value={draft.ip} placeholder={tr("Adresse IP (facultatif)")} inputmode="decimal" />
      <select class="input" bind:value={draft.category}>
        <option value="cz">{tr("Prise")}</option>
        <option value="kg">{tr("Interrupteur")}</option>
        <option value="dj">{tr("Ampoule")}</option>
      </select>
      <button class="btn" type="submit" disabled={!draft.id || draft.key.length !== 16}>{tr("Ajouter")}</button>
    </form>
  {/if}

  {#if found}
    <h2>{tr("Sur le réseau")}</h2>
    {#if !found.length}
      <p class="muted">{tr("Aucun appareil Tuya entendu en 6 secondes. Ils s'annoncent toutes les 5 s quand ils sont branchés et sur le même réseau que Heim.")}</p>
    {:else}
      <ul class="rows">
        {#each found as a (a.id)}
          <li>
            <span class="dot" class:ok={known(a.id)}></span>
            <div class="txt"><strong class="num">{a.id}</strong><span>{a.ip} · v{a.version}{known(a.id) ? tr(" · déjà dans Heim") : tr(" · clé locale à fournir")}</span></div>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}

  <h2>{tr("Obtenir les clés locales")}</h2>
  <ol class="steps">
    <li>{tr("Créez un compte gratuit sur")} <strong>{tr("platform.tuya.com")}</strong>{tr(", puis un « Cloud Project » (région Europe centrale).")}</li>
    <li>{tr("Dans le projet :")} <em>{tr("Devices")}</em> &gt; <em>{tr("Link App Account")}</em>{tr(", et scannez le QR code avec l'application Smart Life ou LSC Smart Connect. Vos appareils apparaissent.")}</li>
    <li>{tr("Sur votre PC :")} <code>pip install tinytuya</code> {tr("puis")} <code>python -m tinytuya wizard</code>{tr(", avec l'« Access ID » et l'« Access Secret » du projet.")}</li>
    <li>{tr("Importez ici le fichier")} <code>devices.json</code> {tr("qu'il a créé. Heim reconnaît tout seul prises, ampoules et interrupteurs.")}</li>
  </ol>
  <p class="muted small">{tr("Si un appareil est aussi relié à Home Assistant par LocalTuya ou Tuya Local, coupez-le là-bas : beaucoup d'appareils Tuya n'acceptent qu'une connexion locale à la fois. Les appareils sur pile (sonnettes, capteurs) dorment et ne parlent qu'au cloud : ils ne marchent pas en local. Changer le Wi-Fi ou réappairer un appareil change sa clé : réimportez alors le fichier.")}</p>
{/if}

<style>
  h2 {
    margin-top: 28px !important;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .check input {
    width: 22px;
    height: 22px;
    accent-color: var(--ink);
  }

  .big {
    margin: 14px 0;
    font-size: 18px;
  }

  .rows {
    list-style: none;
    margin: 0 0 14px;
    padding: 0;
  }

  .rows li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
  }

  .rows li.edit {
    display: block;
  }

  .dot {
    flex: none;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--line);
  }

  .dot.ok {
    background: var(--moss);
  }

  .dot.bad {
    background: var(--alert);
  }

  .txt {
    flex: 1;
    display: grid;
    min-width: 0;
  }

  .txt span {
    font-size: 14px;
    color: var(--muted);
    overflow-wrap: anywhere;
  }

  .mini {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--muted);
  }

  form,
  .manual {
    display: grid;
    gap: 10px;
    max-width: 460px;
    margin: 10px 0;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }

  label.btn {
    cursor: pointer;
  }

  .steps {
    padding-left: 22px;
    display: grid;
    gap: 8px;
  }

  code {
    font-size: 14px;
    padding: 1px 5px;
    border-radius: 5px;
    background: var(--surface-2);
  }

  .small {
    font-size: 14px;
  }
</style>
