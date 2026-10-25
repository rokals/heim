<script>
  import Icon from '../Icon.svelte';
  import { onMount } from 'svelte';
  import { api, toast } from '../../lib/store.svelte.js';

  let info = $state(null);
  let up = $state(null);
  let repo = $state('');
  let busy = $state(false);
  let restarting = $state(false);

  onMount(async () => {
    info = await api('/system').catch(() => null);
    await loadUpdate();
  });

  async function loadUpdate() {
    up = await api('/update').catch(() => null);
    repo = up?.settings?.repo ?? '';
  }

  async function check() {
    busy = true;
    try {
      up.status = await api('/update/check', { method: 'POST' });
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  async function saveSettings(patch) {
    try {
      up.settings = await api('/update/settings', { method: 'PUT', body: { ...$state.snapshot(up.settings), ...patch } });
      toast('Réglage enregistré');
      if (patch.repo !== undefined) check();
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  /** Après un redémarrage, on attend que Heim réponde de nouveau, puis on recharge. */
  async function waitBack() {
    restarting = true;
    await new Promise((r) => setTimeout(r, 4000));
    for (let i = 0; i < 90; i++) {
      try {
        const r = await fetch('/api/health', { cache: 'no-store' });
        if (r.ok) return location.reload();
      } catch {
        /* pas encore */
      }
      await new Promise((r) => setTimeout(r, 2000));
    }
    restarting = false;
    toast("Heim ne répond pas encore : rechargez la page dans un instant", 'alert');
  }

  async function install() {
    if (!confirm(`Installer Heim ${up.status.latest.version} ? Vos données sont sauvegardées avant, et Heim revient tout seul à la version actuelle si la nouvelle ne démarre pas.`)) return;
    busy = true;
    try {
      await api('/update/install', { method: 'POST' });
      waitBack();
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  async function restart(kind) {
    const what = kind === 'reboot' ? 'le Raspberry Pi (environ une minute)' : 'Heim';
    if (!confirm(`Redémarrer ${what} ?`)) return;
    try {
      await api(kind === 'reboot' ? '/system/reboot' : '/system/restart', { method: 'POST' });
      waitBack();
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  let stopped = $state(false);

  async function powerOff() {
    if (!confirm("Éteindre le Raspberry Pi ? Heim s'arrête proprement. Pour le rallumer, il faudra débrancher puis rebrancher son alimentation.")) return;
    try {
      await api('/system/poweroff', { method: 'POST' });
      stopped = true;
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  const fmtDate = (d) => (d ? new Date(d).toLocaleDateString('fr-FR', { day: 'numeric', month: 'long', year: 'numeric' }) : '');

  function uptime(s) {
    const d = Math.floor(s / 86400);
    const h = Math.floor((s % 86400) / 3600);
    const m = Math.floor((s % 3600) / 60);
    return d ? `${d} j ${h} h` : h ? `${h} h ${m} min` : `${m} min`;
  }

  function size(b) {
    if (b == null) return '–';
    return b > 1_048_576 ? `${(b / 1_048_576).toFixed(1)} Mo` : `${Math.round(b / 1024)} Ko`;
  }

  const address = $derived(info?.ip ? `http://${info.ip}${info.port === 80 ? '' : `:${info.port}`}` : null);
</script>

{#if restarting}
  <section class="card restarting" role="status">
    <span class="spin" aria-hidden="true"></span>
    <div><strong>Heim redémarre…</strong><span>La page se rechargera toute seule.</span></div>
  </section>
{/if}

{#if up}
  {@const st = up.status}
  <section class="card update" class:avail={st.available}>
    <h2>Mises à jour</h2>
    {#if st.pending}
      <p class="muted">Version {st.pending} en cours de validation : si elle tient 90 secondes, elle est adoptée.</p>
    {/if}
    {#if st.available && st.latest}
      <p class="big">Heim <strong>{st.latest.version}</strong> est disponible <span class="muted">(vous avez la {st.current})</span></p>
      {#if st.latest.published}<p class="muted">Publiée le {fmtDate(st.latest.published)}</p>{/if}
      {#if st.latest.notes}<pre class="notes">{st.latest.notes}</pre>{/if}
      {#if st.can_install && st.latest.has_asset}
        <button class="btn" disabled={busy || st.installing} onclick={install}>{st.installing ? 'Installation…' : 'Installer la mise à jour'}</button>
      {:else}
        <p class="muted">{st.install_blocker ?? "Pas de fichier pour cette machine dans cette version."}</p>
        {#if st.latest.url?.startsWith('https://github.com/')}<a class="btn quiet" href={st.latest.url} target="_blank" rel="noopener">Voir sur GitHub</a>{/if}
      {/if}
    {:else if st.latest}
      <p>Heim est à jour (version {st.current}).</p>
    {:else}
      <p>Version {st.current}.</p>
    {/if}
    {#if st.error}<p class="error">{st.error}</p>{/if}
    <div class="row">
      <button class="btn quiet" disabled={busy} onclick={check}>{busy ? 'Vérification…' : 'Vérifier maintenant'}</button>
      {#if st.checked}<span class="muted small">Vérifié {new Date(st.checked).toLocaleString('fr-FR', { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' })}</span>{/if}
    </div>
    <details>
      <summary>Réglages des mises à jour</summary>
      <form
        class="repo"
        onsubmit={(e) => {
          e.preventDefault();
          saveSettings({ repo: repo.trim() || null });
        }}
      >
        <label class="field">
          <span>Dépôt GitHub{up.default_repo ? ` (par défaut : ${up.default_repo})` : ''}</span>
          <input class="input" bind:value={repo} placeholder={up.default_repo ?? 'proprietaire/heim'} autocomplete="off" autocapitalize="off" />
        </label>
        <button class="btn quiet" type="submit">Enregistrer</button>
      </form>
      <label class="check"><input type="checkbox" checked={up.settings.auto_check} onchange={(e) => saveSettings({ auto_check: e.currentTarget.checked })} /> Vérifier toute seule deux fois par jour et prévenir les admins</label>
      <label class="check"><input type="checkbox" checked={up.settings.prereleases} onchange={(e) => saveSettings({ prereleases: e.currentTarget.checked })} /> Proposer aussi les versions d'essai</label>
    </details>
  </section>
{/if}

{#if !info}
  <p class="muted">Chargement…</p>
{:else}
  <dl>
    <dt>Version</dt><dd>Heim {info.version}</dd>
    <dt>Adresse sur le réseau</dt><dd>{#if address}<a href={address}>{address}</a>{:else}inconnue{/if}</dd>
    <dt>Nom de la machine</dt><dd>{info.hostname ?? '–'}</dd>
    <dt>En marche depuis</dt><dd class="num">{uptime(info.uptime_s)}</dd>
    <dt>Mémoire utilisée</dt><dd class="num">{info.memory_kib ? `${(info.memory_kib / 1024).toFixed(1)} Mo` : '–'}</dd>
    <dt>Base de données</dt><dd class="num">{size(info.db_bytes)}, {info.entities} entités</dd>
    <dt>Système</dt><dd>{info.os} {info.arch}</dd>
    <dt>Dossier des données</dt><dd class="path">{info.data_dir}</dd>
  </dl>
  <p class="muted">Ouvrez l'adresse ci-dessus sur un téléphone connecté au même Wi-Fi, puis « Ajouter à l'écran d'accueil » pour l'utiliser comme une application.</p>
  {#if info.supervised || info.netsetup}
    <h2>Alimentation</h2>
    <p class="muted">Pour arrêter proprement plutôt que de débrancher : la base est écrite, la carte SD ne risque rien.</p>
    <div class="row power">
      {#if info.supervised}<button class="btn quiet" onclick={() => restart('heim')}><Icon name="restart" size={17} /> Redémarrer Heim</button>{/if}
      {#if info.os === 'linux'}
        <button class="btn quiet" onclick={() => restart('reboot')}><Icon name="restart" size={17} /> Redémarrer le Raspberry Pi</button>
        <button class="btn danger" onclick={powerOff}><Icon name="power" size={17} /> Éteindre</button>
      {/if}
    </div>
  {/if}
{/if}

{#if stopped}
  <div class="off-veil" role="alertdialog">
    <div>
      <Icon name="power" size={34} />
      <strong>Le Raspberry Pi s'éteint</strong>
      <span>Attendez une dizaine de secondes que son voyant vert ne clignote plus, puis vous pouvez le débrancher. Pour le rallumer, rebranchez-le.</span>
    </div>
  </div>
{/if}

<style>
  .power {
    flex-wrap: wrap;
  }

  .off-veil {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    padding: 24px;
    background: color-mix(in srgb, var(--bg) 80%, transparent);
    backdrop-filter: grayscale(1);
  }

  .off-veil div {
    display: grid;
    justify-items: center;
    gap: 10px;
    max-width: 360px;
    padding: 24px;
    border-radius: 22px;
    background: var(--surface);
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.18);
    text-align: center;
  }

  .off-veil span {
    color: var(--muted);
  }

  .card {
    padding: 16px;
    margin-bottom: 24px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .card h2 {
    margin-top: 0 !important;
  }

  .update.avail {
    box-shadow: inset 0 0 0 2px var(--moss);
  }

  .big {
    font-size: 18px;
  }

  .notes {
    max-height: 220px;
    overflow: auto;
    padding: 10px 12px;
    border-radius: 12px;
    background: var(--surface-2);
    font: inherit;
    font-size: 14px;
    white-space: pre-wrap;
  }

  .error {
    color: var(--alert);
    font-weight: 600;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    margin-top: 12px;
  }

  .small {
    font-size: 13px;
  }

  details {
    margin-top: 16px;
  }

  summary {
    cursor: pointer;
    color: var(--muted);
    font-weight: 600;
  }

  .repo {
    display: flex;
    gap: 10px;
    align-items: flex-end;
    flex-wrap: wrap;
    margin-top: 12px;
  }

  .repo .field {
    flex: 1 1 220px;
    margin: 0;
  }

  .check {
    display: flex;
    gap: 10px;
    align-items: center;
    margin-top: 12px;
  }

  .check input {
    width: 20px;
    height: 20px;
    accent-color: var(--ink);
  }

  .restarting {
    display: flex;
    gap: 14px;
    align-items: center;
  }

  .restarting div {
    display: grid;
  }

  .restarting span:not(.spin) {
    color: var(--muted);
  }

  .spin {
    width: 26px;
    height: 26px;
    border: 3px solid var(--line);
    border-top-color: var(--ink);
    border-radius: 50%;
    animation: spin 0.9s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  dl {
    display: grid;
    grid-template-columns: minmax(140px, auto) 1fr;
    gap: 0;
    margin: 0 0 16px;
  }

  dt,
  dd {
    margin: 0;
    padding: 12px 0;
    border-bottom: 1px solid var(--line);
  }

  dt {
    color: var(--muted);
    padding-right: 16px;
  }

  dd {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  a {
    color: inherit;
  }

  .path {
    font-weight: 400;
    font-size: 14px;
  }
</style>
