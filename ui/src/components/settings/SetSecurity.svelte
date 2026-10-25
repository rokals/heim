<script>
  // Réglages de l'alarme, mode simple : un rôle par capteur, des sirènes, des délais, un code.
  import { onMount, untrack } from 'svelte';
  import Icon from '../Icon.svelte';
  import { api, home, loadSecurity, snapshotUrl, toast } from '../../lib/store.svelte.js';
  import { LOG_LABELS, ROLES, securityCandidates, suggestRole } from '../../lib/security.js';

  let cfg = $state(null);
  let roles = $state({});
  let dirty = $state(false);
  let saving = $state(false);
  let testing = $state(false);
  let shots = $state({});
  let code = $state({ current: '', next: '' });

  onMount(async () => {
    await loadSecurity();
    untrack(reset);
  });

  function reset() {
    const c = home.security?.config;
    if (!c) return;
    cfg = structuredClone($state.snapshot(c));
    roles = Object.fromEntries(c.sensors.map((s) => [s.entity_id, { role: s.role, entry: !!s.entry }]));
    dirty = false;
  }

  const all = $derived(Object.values(home.entities));
  const candidates = $derived(securityCandidates(all));
  const sirens = $derived(all.filter((e) => e.domain === 'siren').sort((a, b) => a.name.localeCompare(b.name, 'fr')));
  const lights = $derived(all.filter((e) => e.domain === 'light' && !e.hidden).sort((a, b) => a.name.localeCompare(b.name, 'fr')));
  const log = $derived(home.security?.log ?? []);

  const touch = () => (dirty = true);

  function setRole(id, role) {
    if (!role) delete roles[id];
    else roles[id] = { role, entry: roles[id]?.entry ?? false };
    touch();
  }

  function suggest() {
    let n = 0;
    for (const e of candidates) {
      if (roles[e.entity_id]) continue;
      const r = suggestRole(e);
      if (r) {
        roles[e.entity_id] = { role: r, entry: false };
        n++;
      }
    }
    if (n) touch();
    toast(n ? `${n} capteur(s) proposés : vérifiez, puis enregistrez` : 'Rien à proposer');
  }

  function toggleIn(list, id) {
    const i = cfg[list].indexOf(id);
    if (i === -1) cfg[list].push(id);
    else cfg[list].splice(i, 1);
    touch();
  }

  async function save() {
    saving = true;
    try {
      const body = $state.snapshot(cfg);
      body.sensors = Object.entries($state.snapshot(roles)).map(([entity_id, r]) => ({
        entity_id,
        role: r.role,
        entry: r.role === 'perimeter' && r.entry,
      }));
      body.cameras = body.cameras.filter((c) => c.name.trim() || c.snapshot_url.trim());
      await api('/security/config', { method: 'PUT', body });
      await loadSecurity();
      reset();
      toast('Alarme enregistrée');
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      saving = false;
    }
  }

  async function testSirens() {
    if (dirty) await save();
    testing = true;
    try {
      const r = await api('/security/test_sirens', { method: 'POST', body: { seconds: 3 } });
      toast(r.sirens > 1 ? `${r.sirens} sirènes sonnent 3 secondes` : 'La sirène sonne 3 secondes');
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      setTimeout(() => (testing = false), 3000);
    }
  }

  async function testCamera(c) {
    if (dirty) await save();
    shots[c.name] = 'loading';
    try {
      const r = await api('/security/snapshot', { method: 'POST', body: { camera: c.name } });
      shots[c.name] = r.snapshots[0];
    } catch (e) {
      shots[c.name] = null;
      toast(e.message, 'alert');
    }
  }

  async function saveCode(remove = false) {
    try {
      const r = await api('/security/code', {
        method: 'PUT',
        body: { current: code.current || null, new: remove ? null : code.next },
      });
      cfg.has_code = r.has_code;
      code = { current: '', next: '' };
      await loadSecurity();
      toast(r.has_code ? 'Code enregistré' : 'Code retiré');
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  const fmt = (at) =>
    new Date(at).toLocaleString('fr-FR', { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });
</script>

<p class="muted">Donnez un rôle à chaque capteur. <strong>Absent</strong> surveille tout ; <strong>Nuit</strong> seulement les portes et fenêtres, pour circuler dans la maison.</p>

{#if cfg}
  <h2>Capteurs</h2>
  {#if !candidates.length}
    <p class="muted">Aucun capteur d'ouverture, de mouvement ou de fuite pour l'instant.</p>
  {:else}
    <button class="btn quiet small" onclick={suggest}>Proposer selon le type</button>
    <ul class="list">
      {#each candidates as e (e.entity_id)}
        {@const r = roles[e.entity_id]}
        <li>
          <div class="who">
            <strong>{e.capabilities?.device_name && !e.user_edited ? e.capabilities.device_name : e.name}</strong>
            <span>{e.room || 'Sans pièce'} · {e.capabilities?.kind ?? 'capteur'}{e.state?.available === false ? ' · injoignable' : ''}</span>
          </div>
          <select class="input sel" value={r?.role ?? ''} onchange={(ev) => setRole(e.entity_id, ev.currentTarget.value)} aria-label="Rôle de {e.name}">
            {#each ROLES as o (o.id)}<option value={o.id}>{o.label}</option>{/each}
          </select>
          {#if r?.role === 'perimeter'}
            <label class="check entry">
              <input
                type="checkbox"
                checked={r.entry}
                onchange={(ev) => {
                  roles[e.entity_id].entry = ev.currentTarget.checked;
                  touch();
                }}
              />
              Porte d'entrée : laisse {cfg.entry_delay_s} s pour désactiver
            </label>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  <h2>Sirènes</h2>
  {#if !sirens.length}
    <p class="muted">Aucune sirène Zigbee trouvée. Ajoutez-en une depuis Appareils, « Ajouter un appareil ».</p>
  {:else}
    <ul class="list">
      {#each sirens as s (s.entity_id)}
        <li>
          <label class="check">
            <input type="checkbox" checked={cfg.sirens.includes(s.entity_id)} onchange={() => toggleIn('sirens', s.entity_id)} />
            <Icon name="siren" size={18} /> {s.name}
            {#if s.state?.available === false}<span class="warn">injoignable</span>{/if}
          </label>
        </li>
      {/each}
    </ul>
    <div class="row">
      <button class="btn quiet" disabled={!cfg.sirens.length || testing} onclick={testSirens}>{testing ? 'Ça sonne…' : 'Tester (3 s)'}</button>
    </div>
    <label class="check opt">
      <input type="checkbox" bind:checked={cfg.h24_sirens} onchange={touch} />
      Les capteurs 24 h/24 (fumée, fuite) font aussi sonner les sirènes
    </label>
  {/if}

  <h2>Lumières en cas d'intrusion</h2>
  <p class="muted">Allumées à fond quand l'alarme se déclenche.</p>
  <ul class="list cols">
    {#each lights as l (l.entity_id)}
      <li>
        <label class="check">
          <input type="checkbox" checked={cfg.lights.includes(l.entity_id)} onchange={() => toggleIn('lights', l.entity_id)} />
          {l.name}{#if l.room}<span class="room">{l.room}</span>{/if}
        </label>
      </li>
    {/each}
  </ul>

  <h2>Délais</h2>
  <div class="delays">
    <label class="field"><span>Pour sortir (Absent)</span><span class="unit"><input class="input num" type="number" min="0" max="600" bind:value={cfg.exit_delay_s} oninput={touch} /> s</span></label>
    <label class="field"><span>Pour sortir (Nuit)</span><span class="unit"><input class="input num" type="number" min="0" max="600" bind:value={cfg.night_exit_delay_s} oninput={touch} /> s</span></label>
    <label class="field"><span>Pour désactiver en entrant</span><span class="unit"><input class="input num" type="number" min="0" max="600" bind:value={cfg.entry_delay_s} oninput={touch} /> s</span></label>
    <label class="field"><span>Sirènes pendant</span><span class="unit"><input class="input num" type="number" min="5" max="900" bind:value={cfg.siren_duration_s} oninput={touch} /> s</span></label>
  </div>

  <h2>Caméras</h2>
  <p class="muted">Une photo est prise quand l'alarme se déclenche. Il faut l'adresse d'une image fixe (JPEG) de la caméra, par exemple <code>http://utilisateur:motdepasse@192.168.1.30/snapshot.jpg</code>.</p>
  {#each cfg.cameras as c, i (i)}
    <div class="cam">
      <input class="input" placeholder="Nom (Entrée, Garage…)" bind:value={c.name} oninput={touch} aria-label="Nom de la caméra" />
      <input class="input" placeholder="http://…/snapshot.jpg" bind:value={c.snapshot_url} oninput={touch} aria-label="Adresse de l'image" />
      <div class="row">
        <button class="btn quiet small" disabled={!c.name || !c.snapshot_url || shots[c.name] === 'loading'} onclick={() => testCamera(c)}><Icon name="camera" size={16} /> Prendre une photo</button>
        <button class="btn danger" onclick={() => (cfg.cameras.splice(i, 1), touch())}>Retirer</button>
      </div>
      {#if shots[c.name] && shots[c.name] !== 'loading'}
        <img class="shot" src={snapshotUrl(shots[c.name])} alt="Photo de {c.name}" />
      {/if}
    </div>
  {/each}
  <button class="btn quiet small" onclick={() => (cfg.cameras.push({ name: '', snapshot_url: '' }), touch())}><Icon name="plus" size={16} /> Ajouter une caméra</button>

  {#if dirty}
    <div class="savebar">
      <button class="btn" disabled={saving} onclick={save}>Enregistrer l'alarme</button>
      <button class="btn quiet" onclick={reset}>Annuler</button>
    </div>
  {/if}

  <h2>Code</h2>
  <p class="muted">
    {cfg.has_code
      ? "Un code est demandé pour désactiver l'alarme."
      : "Sans code, toute personne qui ouvre Heim peut désactiver l'alarme. Conseillé si Heim est ouvert sur une tablette dans l'entrée."}
  </p>
  <form
    class="code"
    onsubmit={(e) => {
      e.preventDefault();
      saveCode();
    }}
  >
    {#if cfg.has_code}
      <input class="input" type="password" inputmode="numeric" autocomplete="off" placeholder="Code actuel" bind:value={code.current} />
    {/if}
    <input class="input" type="password" inputmode="numeric" autocomplete="off" placeholder={cfg.has_code ? 'Nouveau code' : 'Code (4 à 12 chiffres)'} bind:value={code.next} />
    <div class="row">
      <button class="btn" type="submit" disabled={code.next.length < 4}>{cfg.has_code ? 'Changer le code' : 'Définir le code'}</button>
      {#if cfg.has_code}<button class="btn danger" type="button" disabled={!code.current} onclick={() => saveCode(true)}>Retirer le code</button>{/if}
    </div>
  </form>
  {#if cfg.has_code}
    <label class="check opt">
      <input type="checkbox" bind:checked={cfg.code_to_arm} onchange={save} />
      Demander aussi le code pour activer
    </label>
  {/if}

  <h2>Historique</h2>
  {#if !log.length}
    <p class="muted">Rien pour l'instant.</p>
  {:else}
    <ul class="log">
      {#each log.slice(0, 60) as l, i (l.at + i)}
        <li class={l.kind}>
          <span class="when num">{fmt(l.at)}</span>
          <span class="what" title={LOG_LABELS[l.kind] ?? l.kind}>{l.message}</span>
          {#if l.snapshot}<a href={snapshotUrl(l.snapshot)} target="_blank" rel="noopener"><img src={snapshotUrl(l.snapshot)} alt="" /></a>{/if}
        </li>
      {/each}
    </ul>
  {/if}
{/if}

<style>
  h2 {
    margin-top: 30px !important;
  }

  .small {
    min-height: 38px;
    padding: 0 14px;
  }

  .list {
    list-style: none;
    margin: 10px 0;
    padding: 0;
  }

  .list li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 12px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
  }

  .cols {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    column-gap: 16px;
  }

  .who {
    flex: 1 1 200px;
    display: grid;
    min-width: 0;
  }

  .who span,
  .room {
    font-size: 14px;
    color: var(--muted);
  }

  .room {
    margin-left: 6px;
  }

  .sel {
    flex: 0 1 190px;
    min-height: 42px;
    padding: 6px 10px;
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

  .entry {
    flex-basis: 100%;
    font-size: 14px;
    color: var(--muted);
  }

  .opt {
    margin-top: 14px;
  }

  .warn {
    font-size: 13px;
    color: var(--alert);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin: 10px 0;
  }

  .delays {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 0 16px;
  }

  .unit {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--muted);
  }

  .unit .input {
    width: 100px;
  }

  .cam {
    display: grid;
    gap: 8px;
    padding: 12px 0;
    border-bottom: 1px solid var(--line);
  }

  .shot {
    max-width: 100%;
    max-height: 220px;
    border-radius: 12px;
    justify-self: start;
  }

  code {
    font-size: 13px;
    word-break: break-all;
  }

  .savebar {
    position: sticky;
    bottom: calc(80px + env(safe-area-inset-bottom));
    display: flex;
    gap: 10px;
    margin: 24px 0 0;
    padding: 10px 0;
  }


  .code {
    display: grid;
    gap: 10px;
    max-width: 360px;
  }

  .log {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .log li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 9px 0;
    border-bottom: 1px solid var(--line);
    font-size: 15px;
  }

  .when {
    flex: none;
    width: 108px;
    color: var(--muted);
    font-size: 13px;
  }

  .what {
    flex: 1;
    min-width: 0;
  }

  .log .triggered .what,
  .log .h24 .what,
  .log .wrong_code .what,
  .log .offline .what {
    color: var(--alert);
    font-weight: 600;
  }

  .log img {
    height: 44px;
    border-radius: 6px;
  }

  @media (min-width: 900px) {
    .savebar {
      bottom: 0;
    }
  }
</style>
