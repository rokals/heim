<script>
  import { tr, locale } from '../../lib/i18n.js';
  // Comptes : le sien (mot de passe, déconnexion) et, pour un admin, ceux de la maison.
  import Icon from '../Icon.svelte';
  import AccessSheet from './AccessSheet.svelte';
  import { api, home, isAdmin, loadMe, logout, toast } from '../../lib/store.svelte.js';

  const ROLES = [
    { id: 'admin', label: tr("Admin"), hint: tr("Règle tout") },
    { id: 'member', label: tr("Membre"), hint: tr("Utilise tout, alarme comprise, sans toucher aux réglages") },
    { id: 'guest', label: tr("Invité"), hint: tr("Lumières, prises et scènes. Pas l’alarme.") },
  ];
  const roleLabel = (r) => ROLES.find((x) => x.id === r)?.label ?? r;

  let users = $state([]);
  let first = $state({ name: '', password: '', confirm: '' });
  let pw = $state({ current: '', next: '' });
  let add = $state({ name: '', role: 'member', password: '' });
  let reset = $state(null);
  let resetPw = $state('');
  let rights = $state(null);

  /** En deux mots, ce que le compte peut faire (tableau imposé, modification…). */
  function accessLine(u) {
    if (u.role === 'admin') return tr("Tous les droits");
    const a = u.access ?? {};
    const bits = [];
    const b = home.boards?.boards?.find((x) => x.id === a.board);
    bits.push(a.board ? tr("Tableau « {v} » imposé", { v: b?.name ?? a.board }) : tr("Tableaux libres"));
    if (a.can_edit && a.can_add) bits.push(tr("modifie et ajoute"));
    else if (a.can_edit) bits.push('modifie');
    else if (a.can_add) bits.push('ajoute');
    if (a.hidden_tabs?.length) bits.push(tr("{length} onglet(s) caché(s)", { length: a.hidden_tabs.length }));
    if (a.readonly?.length) bits.push(tr("{length} en lecture seule", { length: a.readonly.length }));
    return bits.join(' · ');
  }

  const open = $derived(home.me?.open);
  const me = $derived(home.me?.user);

  async function loadUsers() {
    if (!isAdmin() || open) return;
    users = await api('/users').catch(() => []);
  }

  $effect(() => {
    if (home.me) loadUsers();
  });

  async function createFirst(e) {
    e.preventDefault();
    if (first.password !== first.confirm) return toast(tr("Les deux mots de passe ne sont pas identiques"), 'alert');
    try {
      await api('/users', { method: 'POST', body: { name: first.name, password: first.password, role: 'admin' } });
      await loadMe();
      toast(tr("Compte créé : Heim est maintenant protégé"));
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  async function changePassword(e) {
    e.preventDefault();
    try {
      await api('/auth/password', { method: 'POST', body: { current: pw.current, new: pw.next } });
      toast(tr("Mot de passe changé : reconnectez-vous"));
      pw = { current: '', next: '' };
      await logout();
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  async function addUser(e) {
    e.preventDefault();
    try {
      const created = await api('/users', { method: 'POST', body: $state.snapshot(add) });
      toast(tr("Compte « {name} » créé", { name: add.name }));
      const name = add.name.trim();
      add = { name: '', role: 'member', password: '' };
      await loadUsers();
      // Tout de suite ses droits : son tableau, la modification…
      const u = users.find((x) => x.id === created?.user?.id) ?? users.find((x) => x.name === name);
      if (u && u.role !== 'admin') rights = u;
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  async function setRole(u, role) {
    try {
      await api(`/users/${u.id}`, { method: 'PUT', body: { role } });
      toast(`${u.name} : ${roleLabel(role)}`);
    } catch (err) {
      toast(err.message, 'alert');
    }
    loadUsers();
  }

  async function saveReset(e) {
    e.preventDefault();
    try {
      await api(`/users/${reset.id}`, { method: 'PUT', body: { password: resetPw } });
      toast(tr("Nouveau mot de passe pour {name}", { name: reset.name }));
      reset = null;
      resetPw = '';
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  async function remove(u) {
    if (!confirm(tr("Supprimer le compte de {name} ? Ses appareils seront déconnectés.", { name: u.name }))) return;
    try {
      await api(`/users/${u.id}`, { method: 'DELETE' });
      loadUsers();
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  const fmt = (d) => new Date(d).toLocaleDateString(locale(), { day: 'numeric', month: 'short', year: 'numeric' });
</script>

{#if open}
  <section class="card">
    <h2>{tr("Protéger Heim")}</h2>
    <p class="muted">{tr("Aujourd'hui, toute personne sur le réseau de la maison peut tout faire. Créez votre compte admin : ensuite, chacun aura le sien (admin, membre ou invité).")}</p>
    <form onsubmit={createFirst}>
      <label class="field"><span>{tr("Votre prénom")}</span><input class="input" bind:value={first.name} autocomplete="username" /></label>
      <label class="field"><span>{tr("Mot de passe (8 caractères ou plus)")}</span><input class="input" type="password" bind:value={first.password} autocomplete="new-password" /></label>
      <label class="field"><span>{tr("Le même, encore")}</span><input class="input" type="password" bind:value={first.confirm} autocomplete="new-password" /></label>
      <button class="btn" type="submit" disabled={!first.name.trim() || first.password.length < 8}>{tr("Créer mon compte admin")}</button>
    </form>
  </section>
{:else if me}
  <section class="card who">
    <span class="avatar" aria-hidden="true">{me.name.slice(0, 1).toUpperCase()}</span>
    <div class="txt"><strong>{me.name}</strong><span>{roleLabel(me.role)}</span></div>
    <button class="btn quiet" onclick={logout}>{tr("Se déconnecter")}</button>
  </section>

  <h2>{tr("Mon mot de passe")}</h2>
  <form class="pw" onsubmit={changePassword}>
    <input class="input" type="password" placeholder={tr("Mot de passe actuel")} bind:value={pw.current} autocomplete="current-password" />
    <input class="input" type="password" placeholder={tr("Nouveau (8 caractères ou plus)")} bind:value={pw.next} autocomplete="new-password" />
    <button class="btn quiet" type="submit" disabled={!pw.current || pw.next.length < 8}>{tr("Changer")}</button>
  </form>
  {#if home.me.sessions?.length > 1}
    <p class="muted">{tr("Connecté sur {length} appareils. Changer de mot de passe les déconnecte tous.", { length: home.me.sessions.length })}</p>
  {/if}
{:else}
  <p class="muted">{tr("Heim est protégé par un code d'accès technique (HEIM_TOKEN). Les comptes se créent depuis un navigateur connecté avec ce code.")}</p>
{/if}

{#if isAdmin() && !open && users.length}
  <h2>{tr("Les comptes de la maison")}</h2>
  <ul class="users">
    {#each users as u (u.id)}
      <li>
        <span class="avatar small" aria-hidden="true">{u.name.slice(0, 1).toUpperCase()}</span>
        <div class="txt"><strong>{u.name}{u.id === me?.id ? tr(' (vous)') : ''}</strong><span>{accessLine(u)}</span></div>
        <select class="input sel" value={u.role} onchange={(e) => setRole(u, e.currentTarget.value)} aria-label={tr("Rôle de {name}", { name: u.name })} disabled={u.id === me?.id}>
          {#each ROLES as r (r.id)}<option value={r.id}>{r.label}</option>{/each}
        </select>
        {#if u.id !== me?.id}
          {#if u.role !== 'admin'}<button class="mini" aria-label={tr("Droits de {name}", { name: u.name })} title={tr("Droits : tableau, modification, onglets…")} onclick={() => (rights = u)}><Icon name="shield" size={16} /></button>{/if}
          <button class="mini" aria-label={tr("Nouveau mot de passe pour {name}", { name: u.name })} onclick={() => ((reset = u), (resetPw = ''))}><Icon name="pencil" size={16} /></button>
          <button class="mini" aria-label={tr("Supprimer {name}", { name: u.name })} onclick={() => remove(u)}><Icon name="close" size={16} /></button>
        {/if}
      </li>
      {#if reset?.id === u.id}
        <li class="reset">
          <form onsubmit={saveReset}>
            <input class="input" type="password" placeholder={tr("Nouveau mot de passe pour {name}", { name: u.name })} bind:value={resetPw} autocomplete="new-password" />
            <button class="btn quiet" type="submit" disabled={resetPw.length < 8}>{tr("Enregistrer")}</button>
            <button class="btn danger" type="button" onclick={() => (reset = null)}>{tr("Annuler")}</button>
          </form>
        </li>
      {/if}
    {/each}
  </ul>
  <dl class="roles">
    {#each ROLES as r (r.id)}<div><dt>{r.label}</dt><dd>{r.hint}</dd></div>{/each}
  </dl>

  <h2>{tr("Ajouter quelqu'un")}</h2>
  <form class="add" onsubmit={addUser}>
    <input class="input" placeholder={tr("Prénom")} bind:value={add.name} autocomplete="off" />
    <select class="input" bind:value={add.role} aria-label={tr("Rôle")}>
      {#each ROLES as r (r.id)}<option value={r.id}>{r.label}</option>{/each}
    </select>
    <input class="input" type="password" placeholder={tr("Mot de passe (8 caractères ou plus)")} bind:value={add.password} autocomplete="new-password" />
    <button class="btn" type="submit" disabled={!add.name.trim() || add.password.length < 8}>{tr("Créer le compte")}</button>
  </form>
{/if}

{#if rights}
  <AccessSheet user={rights} onclose={() => (rights = null)} onsaved={loadUsers} />
{/if}

<style>
  .card {
    padding: 16px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .card h2 {
    margin-top: 0 !important;
  }

  h2 {
    margin-top: 28px !important;
  }

  .who {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .avatar {
    display: inline-grid;
    place-items: center;
    flex: none;
    width: 46px;
    height: 46px;
    border-radius: 50%;
    background: var(--ink);
    color: var(--bg);
    font-weight: 600;
    font-size: 19px;
  }

  .avatar.small {
    width: 36px;
    height: 36px;
    font-size: 16px;
  }

  .txt {
    flex: 1;
    display: grid;
    min-width: 0;
  }

  .txt span {
    font-size: 14px;
    color: var(--muted);
  }

  .pw,
  .add,
  .reset form {
    display: grid;
    gap: 10px;
    max-width: 420px;
  }

  .users {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .users li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
  }

  .users li.reset {
    display: block;
  }

  .sel {
    flex: 0 0 120px;
    min-height: 40px;
    padding: 6px 10px;
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

  .roles {
    margin: 14px 0 0;
    display: grid;
    gap: 4px;
    font-size: 14px;
    color: var(--muted);
  }

  .roles div {
    display: flex;
    gap: 8px;
  }

  .roles dt {
    font-weight: 600;
    min-width: 64px;
  }

  .roles dd {
    margin: 0;
  }
</style>
