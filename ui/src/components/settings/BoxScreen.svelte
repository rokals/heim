<script>
  import { tr } from '../../lib/i18n.js';
  // L'écran tactile branché sur les broches de la box (plugin « Écran 3,5" ») : modèle,
  // orientation, tableau affiché, mise en veille, réglage du tactile.
  import Icon from '../Icon.svelte';
  import { api, home, toast } from '../../lib/store.svelte.js';

  let st = $state(null);
  let busy = $state(false);
  const boards = $derived(home.boards?.boards ?? []);

  async function load() {
    try {
      st = await api('/screen/local');
    } catch {
      st = null;
    }
  }
  $effect(() => {
    load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  });

  async function save(body, done) {
    busy = true;
    try {
      st = await api('/screen/local', { method: 'PUT', body });
      if (done) toast(done);
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      busy = false;
    }
  }

  async function reboot() {
    if (!confirm(tr("Redémarrer la box maintenant ? Heim revient dans une minute environ."))) return;
    try {
      await api('/system/reboot', { method: 'POST' });
      toast(tr("La box redémarre"));
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  const tone = $derived(!st ? '' : st.framebuffer ? 'ok' : st.needs_reboot ? 'wait' : st.pending ? 'wait' : 'bad');
  const line = $derived(
    !st
      ? ''
      : st.framebuffer
        ? st.touch
          ? tr("Écran actif, tactile prêt")
          : tr("Écran actif, tactile introuvable")
        : st.pending
          ? tr("Réglage du Raspberry Pi en cours…")
          : st.needs_reboot
            ? tr("Réglé : redémarrez la box pour allumer l'écran")
            : st.result === 'failed'
              ? tr("Réglage impossible : {m}", { m: st.message ?? '' })
              : tr("En attente de la mise à jour système"),
  );
</script>

{#if st?.settings?.enabled}
  <section class="box">
    <h3><Icon name="grid" size={20} /> {tr("Écran branché sur la box")}</h3>
    <p class="state {tone}"><span class="led"></span>{line}</p>
    {#if st.needs_reboot && !st.framebuffer}
      <button class="btn" onclick={reboot}>{tr("Redémarrer la box")}</button>
    {/if}

    <label class="field">
      <span>{tr("Modèle")}</span>
      <select class="input" value={st.settings.model} disabled={busy} onchange={(e) => save({ model: e.currentTarget.value })}>
        {#each st.models as m (m.id)}<option value={m.id}>{tr(m.name)}</option>{/each}
      </select>
    </label>
    <label class="field">
      <span>{tr("Orientation")}</span>
      <select class="input" value={String(st.settings.rotate)} disabled={busy} onchange={(e) => save({ rotate: Number(e.currentTarget.value) }, tr("Orientation enregistrée : redémarrez la box"))}>
        <option value="90">{tr("Paysage")}</option>
        <option value="270">{tr("Paysage retourné")}</option>
        <option value="0">{tr("Portrait")}</option>
        <option value="180">{tr("Portrait retourné")}</option>
      </select>
    </label>
    <label class="field">
      <span>{tr("Tableau affiché")}</span>
      <select class="input" value={st.settings.board ?? ''} disabled={busy} onchange={(e) => save({ board: e.currentTarget.value })}>
        <option value="">{tr("Le premier tableau")}</option>
        {#each boards as b (b.id)}<option value={b.id}>{b.name}</option>{/each}
      </select>
    </label>
    <label class="field">
      <span>{tr("Mise en veille")}</span>
      <select class="input" value={String(st.settings.sleep_min)} disabled={busy} onchange={(e) => save({ sleep_min: Number(e.currentTarget.value) })}>
        <option value="1">{tr("Après 1 minute")}</option>
        <option value="5">{tr("Après 5 minutes")}</option>
        <option value="15">{tr("Après 15 minutes")}</option>
        <option value="60">{tr("Après 1 heure")}</option>
        <option value="0">{tr("Jamais")}</option>
      </select>
    </label>
    <button class="btn quiet" disabled={busy || !st.framebuffer} onclick={() => save({ recalibrate: true }, tr("Touchez les croix sur l'écran"))}>
      {tr("Refaire le réglage du tactile")}
    </button>
  </section>
{/if}

<style>
  .box {
    display: grid;
    gap: 10px;
    margin: 0 0 20px;
    padding: 14px 16px;
    border: 1px solid var(--line);
    border-radius: 16px;
  }

  h3 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    font-size: 17px;
  }

  .state {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    color: var(--muted);
  }

  .led {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--muted);
  }

  .ok .led {
    background: var(--moss);
  }

  .wait .led {
    background: #d9a441;
  }

  .bad .led {
    background: var(--alert);
  }

  .field {
    display: grid;
    gap: 4px;
  }

  .btn {
    justify-self: start;
  }
</style>
