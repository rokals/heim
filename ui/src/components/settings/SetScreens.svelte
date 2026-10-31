<script>
  import { tr } from '../../lib/i18n.js';
  // Écrans : chaque téléphone, tablette ou ordinateur qui a ouvert Heim. Depuis ici, on lui
  // donne un nom, un tableau, des limites, le mode tablette murale… et il suit en direct.
  import Sheet from '../Sheet.svelte';
  import Icon from '../Icon.svelte';
  import LookEditor from './LookEditor.svelte';
  import BoxScreen from './BoxScreen.svelte';
  import { api, deviceId, home, toast } from '../../lib/store.svelte.js';
  import { cleanLook, effectiveLook } from '../../lib/theme.js';
  import { ago } from '../../lib/house.js';

  let screens = $state([]);
  let loaded = $state(false);
  let open = $state(null);
  const me = deviceId();
  const boards = $derived(home.boards?.boards ?? []);
  const current = $derived(screens.find((s) => s.id === open) ?? null);

  async function load() {
    try {
      screens = await api('/screens');
    } catch {
      screens = [];
    }
    loaded = true;
  }

  // Un écran se connecte, se déconnecte ou change : la liste se relit (un peu groupé).
  let timer;
  $effect(() => {
    home.screensTick;
    clearTimeout(timer);
    timer = setTimeout(load, loaded ? 400 : 0);
    return () => clearTimeout(timer);
  });

  async function patch(id, body) {
    const i = screens.findIndex((s) => s.id === id);
    if (i >= 0) screens[i] = { ...screens[i], ...body };
    try {
      const saved = await api(`/screens/${encodeURIComponent(id)}`, { method: 'PUT', body });
      if (i >= 0) screens[i] = { ...screens[i], ...saved };
    } catch (e) {
      toast(e.message, 'alert');
      load();
    }
  }

  async function identify(id) {
    try {
      await api(`/screens/${encodeURIComponent(id)}/identify`, { method: 'POST' });
      toast(tr("Son nom s’affiche en grand pendant quelques secondes"));
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  async function forget(s) {
    if (!confirm(tr("Oublier « {name} » ? Il réapparaîtra, sans ses réglages, s’il se reconnecte.", { name: s.name }))) return;
    try {
      await api(`/screens/${encodeURIComponent(s.id)}`, { method: 'DELETE' });
      open = null;
      load();
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  const TABS = [
    ['appareils', tr("Appareils")],
    ['scenes', tr("Scènes")],
    ['automatismes', tr("Automatismes")],
    ['reglages', tr("Réglages")],
  ];
  const boardName = (id) => boards.find((b) => b.id === id)?.name;

  function summary(s) {
    const bits = [];
    if (s.kiosk) bits.push(tr("tablette murale"));
    if (s.board) bits.push(tr("tableau « {v} »{v2}", { v: boardName(s.board) ?? s.board, v2: s.lock_board ? tr(" imposé") : '' }));
    if (!s.can_edit || !s.can_add) bits.push(tr("modification limitée"));
    return bits.join(' · ');
  }

  function toggleTab(s, id) {
    const t = new Set(s.hidden_tabs ?? []);
    if (t.has(id)) t.delete(id);
    else t.add(id);
    patch(s.id, { hidden_tabs: [...t] });
  }
</script>

<BoxScreen />

<p class="muted">
  {tr("Chaque téléphone, tablette ou ordinateur qui ouvre Heim apparaît ici. Donnez-lui un nom, un tableau, bloquez la modification ou passez-le en tablette murale : il se met à jour aussitôt, sans y toucher.")}
</p>

<ul class="list">
  {#each screens as s (s.id)}
    <li>
      <button class="row" onclick={() => (open = s.id)}>
        <span class="ic" class:online={s.online}><Icon name={/Ordinateur|Mac/.test(s.kind ?? '') ? 'grid' : 'sensor'} size={20} /></span>
        <span class="txt">
          <strong>{s.name}{#if s.id === me}<em class="me">{tr("cet écran")}</em>{/if}</strong>
          <span>
            {s.online ? tr("Connecté") : s.last_seen ? tr("Vu {ago}", { ago: ago(s.last_seen, home.now) }) : tr("Jamais vu")}{s.kind ? ` · ${s.kind}` : ''}{s.user ? ` · ${s.user}` : ''}
          </span>
          {#if summary(s)}<span class="sum">{summary(s)}</span>{/if}
        </span>
        <span class="chev" aria-hidden="true">›</span>
      </button>
    </li>
  {:else}
    <li class="none">{loaded ? tr("Aucun écran pour l’instant.") : tr("Chargement…")}</li>
  {/each}
</ul>

{#if current}
  {@const s = current}
  <Sheet title={s.name} onclose={() => (open = null)}>
    <p class="muted small">
      {s.online ? tr("Connecté") : s.last_seen ? tr("Vu {ago}", { ago: ago(s.last_seen, home.now) }) : ''}{s.kind ? ` · ${s.kind}` : ''}{s.user ? tr("· compte {user}", { user: s.user }) : tr(" · sans compte")}
    </p>
    <div class="quick">
      <button class="btn quiet" onclick={() => identify(s.id)} disabled={!s.online}><Icon name="bell" size={16} /> {tr("Identifier")}</button>
    </div>

    <label class="field">
      <span>{tr("Nom")}</span>
      <input class="input" value={s.name} onchange={(e) => patch(s.id, { name: e.currentTarget.value })} placeholder={tr("Tablette cuisine")} />
    </label>

    <h3>{tr("Tableau")}</h3>
    <label class="field">
      <span>{tr("Ouvre sur")}</span>
      <select class="input" value={s.board ?? ''} onchange={(e) => patch(s.id, { board: e.currentTarget.value || null })}>
        <option value="">{tr("Le tableau habituel")}</option>
        {#each boards as b (b.id)}<option value={b.id}>{b.name}{b.hidden ? tr(" (caché)") : ''}</option>{/each}
      </select>
    </label>
    <label class="check">
      <input type="checkbox" checked={s.lock_board} disabled={!s.board} onchange={(e) => patch(s.id, { lock_board: e.currentTarget.checked })} />
      <span><strong>{tr("Ne pas changer de tableau")}</strong><em>{tr("Pas de glissement ni d’onglets sur cet écran")}</em></span>
    </label>
    <label class="check">
      <input type="checkbox" checked={s.can_edit} onchange={(e) => patch(s.id, { can_edit: e.currentTarget.checked })} />
      <span><strong>{tr("Modifier les tuiles")}</strong><em>{tr("Déplacer, agrandir, régler, retirer")}</em></span>
    </label>
    <label class="check">
      <input type="checkbox" checked={s.can_add} onchange={(e) => patch(s.id, { can_add: e.currentTarget.checked })} />
      <span><strong>{tr("Ajouter des tuiles")}</strong></span>
    </label>
    <p class="muted small">{tr("Ces limites s’ajoutent à celles du compte connecté sur l’écran.")}</p>

    <h3>{tr("Tablette murale")}</h3>
    <label class="check">
      <input type="checkbox" checked={s.kiosk} onchange={(e) => patch(s.id, { kiosk: e.currentTarget.checked })} />
      <span><strong>{tr("Mode tablette murale")}</strong><em>{tr("Plein écran, sans barre ni onglets : juste le tableau")}</em></span>
    </label>
    <label class="check">
      <input type="checkbox" checked={s.keep_awake} onchange={(e) => patch(s.id, { keep_awake: e.currentTarget.checked })} />
      <span><strong>{tr("Garder l’écran allumé")}</strong><em>{tr("Il faut une adresse https (Tailscale) sur la plupart des navigateurs")}</em></span>
    </label>
    <div class="field">
      <span>{tr("Revenir à son tableau sans toucher l’écran depuis")}</span>
      <div class="seg wrap">
        {#each [0, 2, 5, 10, 30] as m (m)}
          <button class:on={(s.return_min ?? 0) === m} onclick={() => patch(s.id, { return_min: m })}>{m ? `${m} min` : tr("Jamais")}</button>
        {/each}
      </div>
    </div>

    {#if !s.kiosk}
      <div class="field">
        <span>{tr("Onglets cachés sur cet écran")}</span>
        <div class="seg wrap">
          {#each TABS as [id, label] (id)}
            <button class:on={s.hidden_tabs?.includes(id)} onclick={() => toggleTab(s, id)}>{label}</button>
          {/each}
        </div>
      </div>
    {/if}

    <h3>{tr("Apparence")}</h3>
    <label class="check">
      <input
        type="checkbox"
        checked={Boolean(s.appearance)}
        onchange={(e) => patch(s.id, { appearance: e.currentTarget.checked ? {} : null })}
      />
      <span><strong>{tr("Une apparence à lui")}</strong><em>{tr("Sinon, celle du compte connecté")}</em></span>
    </label>
    {#if s.appearance}
      <LookEditor look={s.appearance} base={effectiveLook(home.look?.house)} onchange={(l) => patch(s.id, { appearance: cleanLook(l) })} />
    {/if}

    <div class="actions">
      <button class="btn" onclick={() => (open = null)}>{tr("Terminé")}</button>
      <button class="btn danger" onclick={() => forget(s)}>{tr("Oublier cet écran")}</button>
    </div>
  </Sheet>
{/if}

<style>
  .list {
    list-style: none;
    padding: 0;
    margin: 12px 0 0;
  }

  .list li {
    border-bottom: 1px solid var(--line);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 12px 2px;
    text-align: left;
  }

  .ic {
    position: relative;
    flex: none;
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    border-radius: 12px;
    background: var(--surface-2);
    color: var(--muted);
  }

  .ic.online::after {
    content: '';
    position: absolute;
    right: -2px;
    bottom: -2px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--moss);
    box-shadow: 0 0 0 2px var(--bg);
  }

  .txt {
    flex: 1;
    display: grid;
    min-width: 0;
  }

  .txt span {
    font-size: 13px;
    color: var(--muted);
  }

  .txt .sum {
    color: var(--ink);
  }

  .me {
    margin-left: 8px;
    padding: 1px 8px;
    border-radius: 9px;
    font-style: normal;
    font-size: 12px;
    background: var(--ember);
    color: var(--ember-ink);
  }

  .chev {
    font-size: 22px;
    color: var(--muted);
  }

  .none {
    padding: 14px 0;
    color: var(--muted);
  }

  .small {
    font-size: 13px;
    margin: 0 0 8px;
  }

  .quick {
    display: flex;
    gap: 8px;
    margin-bottom: 4px;
  }

  h3 {
    margin: 20px 0 4px;
    font-size: 17px;
  }

  .check {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 0;
  }

  .check input {
    width: 22px;
    height: 22px;
    margin-top: 1px;
    accent-color: var(--ink);
    flex: none;
  }

  .check span {
    display: grid;
  }

  .check em {
    font-style: normal;
    font-size: 13px;
    color: var(--muted);
  }

  .seg {
    display: flex;
    gap: 6px;
  }

  .seg.wrap {
    flex-wrap: wrap;
  }

  .seg button {
    min-height: 42px;
    padding: 0 14px;
    border-radius: var(--radius-chip);
    border: 1.5px solid var(--line);
    font-weight: 600;
  }

  .seg button.on {
    background: var(--ink);
    border-color: var(--ink);
    color: var(--bg);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 18px;
  }
</style>
