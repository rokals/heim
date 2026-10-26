<script>
  import { tr } from '../../lib/i18n.js';
  // La position sert uniquement à calculer le lever et le coucher du soleil, sur place.
  import { onMount } from 'svelte';
  import { api, toast } from '../../lib/store.svelte.js';

  let lat = $state(46.6);
  let lon = $state(2.4);

  onMount(async () => {
    const l = await api('/settings/location').catch(() => null);
    if (l) {
      lat = l.latitude;
      lon = l.longitude;
    }
  });

  async function save(e) {
    e?.preventDefault();
    try {
      await api('/settings/location', { method: 'PUT', body: { latitude: Number(lat), longitude: Number(lon) } });
      toast(tr("Lieu enregistré"));
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  function locate() {
    if (!navigator.geolocation) {
      toast(tr("Ce navigateur ne donne pas sa position. Saisissez-la à la main."), 'alert');
      return;
    }
    navigator.geolocation.getCurrentPosition(
      (p) => {
        lat = Math.round(p.coords.latitude * 100) / 100;
        lon = Math.round(p.coords.longitude * 100) / 100;
        save();
      },
      () => toast(tr("Position refusée ou indisponible (souvent le cas hors https). Saisissez-la à la main."), 'alert'),
    );
  }
</script>

<p class="muted">{tr("Utilisée seulement pour savoir quand il fait nuit, sans passer par Internet. Deux décimales suffisent : inutile d'être précis.")}</p>

<form onsubmit={save}>
  <div class="two">
    <label class="field">
      <span>{tr("Latitude")}</span>
      <input class="input num" type="number" step="0.01" min="-90" max="90" bind:value={lat} />
    </label>
    <label class="field">
      <span>{tr("Longitude")}</span>
      <input class="input num" type="number" step="0.01" min="-180" max="180" bind:value={lon} />
    </label>
  </div>
  <div class="row">
    <button class="btn" type="submit">{tr("Enregistrer")}</button>
    <button class="btn quiet" type="button" onclick={locate}>{tr("Utiliser ma position")}</button>
  </div>
</form>

<style>
  .two {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 0 12px;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
