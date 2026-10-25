<script>
  // Une sauvegarde = tout Heim dans un seul fichier : appareils renommés, pièces, scènes,
  // automatismes, accueil, réglages.
  import { onMount } from 'svelte';
  import { api, toast } from '../../lib/store.svelte.js';

  let busy = $state(false);
  let restored = $state(false);
  let nightly = $state(null);

  async function loadNightly() {
    nightly = await api('/backups').catch(() => null);
  }

  onMount(loadNightly);

  async function backupNow() {
    busy = true;
    try {
      await api('/backups', { method: 'POST' });
      toast('Sauvegarde du jour faite');
      await loadNightly();
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  async function fetchFile(url, fallback) {
    const t = token();
    const res = await fetch(url, { headers: t ? { Authorization: 'Bearer ' + t } : {} });
    if (!res.ok) throw new Error((await res.json().catch(() => null))?.error || `Erreur ${res.status}`);
    const blob = await res.blob();
    const name = /filename="([^"]+)"/.exec(res.headers.get('content-disposition') || '')?.[1] || fallback;
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = name;
    a.click();
    URL.revokeObjectURL(a.href);
  }

  async function downloadNightly(name) {
    try {
      await fetchFile(`/api/backups/${encodeURIComponent(name)}`, name);
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  const day = (d) => new Date(d + 'T12:00:00').toLocaleDateString('fr-FR', { weekday: 'long', day: 'numeric', month: 'long' });
  const kb = (b) => (b > 1_048_576 ? `${(b / 1_048_576).toFixed(1)} Mo` : `${Math.round(b / 1024)} Ko`);

  function token() {
    try {
      return localStorage.getItem('heim_token') || '';
    } catch {
      return '';
    }
  }

  async function download() {
    busy = true;
    try {
      const t = token();
      const res = await fetch('/api/backup', { headers: t ? { Authorization: 'Bearer ' + t } : {} });
      if (!res.ok) throw new Error((await res.json().catch(() => null))?.error || `Erreur ${res.status}`);
      const blob = await res.blob();
      const name = /filename="([^"]+)"/.exec(res.headers.get('content-disposition') || '')?.[1] || 'heim.db';
      const a = document.createElement('a');
      a.href = URL.createObjectURL(blob);
      a.download = name;
      a.click();
      URL.revokeObjectURL(a.href);
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  async function restore(e) {
    const file = e.currentTarget.files?.[0];
    if (!file) return;
    if (!confirm(`Remplacer toute la configuration de Heim par « ${file.name} » au prochain redémarrage ?`)) return;
    busy = true;
    try {
      const t = token();
      const res = await fetch('/api/restore', {
        method: 'POST',
        headers: { 'Content-Type': 'application/octet-stream', ...(t ? { Authorization: 'Bearer ' + t } : {}) },
        body: file,
      });
      const data = await res.json().catch(() => null);
      if (!res.ok) throw new Error(data?.error || `Erreur ${res.status}`);
      restored = true;
    } catch (err) {
      toast(err.message, 'alert');
    } finally {
      busy = false;
    }
  }
</script>

<p class="muted">Un seul fichier contient tout : noms et pièces des appareils, scènes, automatismes, accueil et réglages. Gardez-en une copie avant une grosse modification.</p>

<button class="btn" onclick={download} disabled={busy}>Télécharger une sauvegarde</button>

<h2>Chaque nuit</h2>
<p class="muted">Heim se sauvegarde tout seul vers 3 h du matin, sur la carte SD, et garde les {nightly?.keep ?? 7} dernières. Téléchargez-en une de temps en temps pour l'avoir ailleurs (ordinateur, NAS).</p>
{#if nightly?.backups?.length}
  <ul class="rows">
    {#each nightly.backups as b (b.name)}
      <li>
        <span><strong>{day(b.date)}</strong> <span class="muted num">{kb(b.bytes)}</span></span>
        <button class="btn quiet" onclick={() => downloadNightly(b.name)}>Télécharger</button>
      </li>
    {/each}
  </ul>
{:else if nightly}
  <p class="muted">Pas encore de sauvegarde de nuit : la première se fait quelques minutes après le démarrage.</p>
{/if}
<button class="btn quiet" onclick={backupNow} disabled={busy}>Sauvegarder maintenant</button>

<h2>Restaurer</h2>
{#if restored}
  <p class="ok">Sauvegarde reçue. Redémarrez Heim pour la charger ; l'ancienne configuration est gardée à côté (heim.db.bak).</p>
{:else}
  <label class="btn quiet file">
    Choisir un fichier de sauvegarde
    <input type="file" accept=".db,application/octet-stream" onchange={restore} disabled={busy} />
  </label>
{/if}

<style>
  .rows {
    list-style: none;
    padding: 0;
    margin: 10px 0;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
    overflow: hidden;
  }

  .rows li {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    padding: 8px 8px 8px 14px;
  }

  .rows li + li {
    border-top: 1px solid var(--line);
  }

  .rows strong {
    text-transform: capitalize;
  }

  h2 {
    margin-top: 28px !important;
  }

  .file {
    position: relative;
    overflow: hidden;
  }

  .file input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
  }

  .ok {
    color: var(--moss);
    font-weight: 600;
  }
</style>
