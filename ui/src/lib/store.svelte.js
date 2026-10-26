import { roomNames } from './house.js';

// État partagé de l'interface : la liste des entités tenue à jour par le WebSocket,
// plus quelques utilitaires pour appeler l'API.

const KEY_TOKEN = 'heim_token';
const KEY_DEVICE = 'heim_device';
const KEY_DEVICE_NAME = 'heim_device_name';
const KEY_BOARD = 'heim_board';
const PENDING_MS = 15_000;

function storage(key, value) {
  try {
    if (value === undefined) return localStorage.getItem(key);
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    // Navigation privée ou stockage bloqué : l'interface marche quand même, sans mémoire.
  }
  return null;
}

export const home = $state({
  entities: {},
  loaded: false,
  connected: false,
  needToken: false,
  /** Écran de connexion : la liste des prénoms */
  needLogin: false,
  loginUsers: [],
  /** Le compte connecté : { user, role, open } */
  me: null,
  /** Premier démarrage : { needed, netsetup, network } (null : pas besoin de l'assistant) */
  setup: null,
  /** La maison : { name, floors, rooms: [{ name, floor, icon }] } */
  layout: null,
  /** entity_id -> instant de la commande, tant que l'appareil n'a pas confirmé */
  pending: {},
  toasts: [],
  integrations: {},
  targetId: null,
  now: Date.now(),
  /**
   * Les tableaux de bord que ce compte voit, et ses droits :
   * { boards: [{ id, name, icon, hidden, auto, sections }], home, locked, can_edit, can_add, can_manage }
   */
  boards: null,
  /** Le tableau affiché (identifiant). */
  board: storage(KEY_BOARD),
  /** Réglages de cet écran donnés par l'admin (Réglages > Écrans), ou null. */
  screen: null,
  /** Apparence : celle de la maison et celle du compte. */
  look: { house: {}, mine: null },
  /** « Identifier » demandé depuis un autre téléphone : le nom de l'écran s'affiche en grand. */
  identify: false,
  /** Change à chaque nouvelle d'un écran (la page Écrans se relit). */
  screensTick: 0,
  /** Lever et coucher du soleil (carte Soleil). */
  sun: null,
  automations: [],
  lastRun: {},
  /** Alarme : { status, config, log, snapshots } (null tant que pas chargé) */
  security: null,
  /** Appairage Zigbee : { open, until, version, events } */
  pairing: { open: false, until: null, version: null, events: [] },
});

let token = new URLSearchParams(location.search).get('token') || storage(KEY_TOKEN) || '';

export function deviceId() {
  let id = storage(KEY_DEVICE);
  if (!id) {
    id = 'app_' + Math.random().toString(36).slice(2, 8);
    storage(KEY_DEVICE, id);
  }
  return id;
}

export function deviceLabel() {
  const saved = storage(KEY_DEVICE_NAME);
  if (saved) return saved;
  const ua = navigator.userAgent;
  if (/iPhone/.test(ua)) return 'iPhone';
  if (/iPad/.test(ua)) return 'iPad';
  if (/Android/.test(ua)) return /Mobile/.test(ua) ? 'Téléphone Android' : 'Tablette Android';
  if (/Windows/.test(ua)) return 'Ordinateur Windows';
  if (/Mac/.test(ua)) return 'Mac';
  return 'Navigateur';
}

/** Ce que le navigateur dit être, pour la page Écrans (le nom, lui, peut être changé). */
export function deviceKind() {
  const ua = navigator.userAgent;
  if (/iPhone/.test(ua)) return 'iPhone';
  if (/iPad|Macintosh.*Mobile/.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1)) return 'iPad';
  if (/Android/.test(ua)) return /Mobile/.test(ua) ? 'Téléphone Android' : 'Tablette Android';
  if (/Windows/.test(ua)) return 'Ordinateur Windows';
  if (/Mac/.test(ua)) return 'Mac';
  return 'Navigateur';
}

export function rememberDeviceLabel(name) {
  storage(KEY_DEVICE_NAME, name);
}

export function toast(text, kind = 'info') {
  const id = Math.random();
  home.toasts.push({ id, text, kind });
  setTimeout(() => {
    home.toasts = home.toasts.filter((t) => t.id !== id);
  }, 4500);
}

export async function api(path, { method = 'GET', body } = {}) {
  const headers = {};
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  if (token) headers.Authorization = 'Bearer ' + token;
  const res = await fetch('/api' + path, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (res.status === 401 && !path.startsWith('/auth/login')) {
    await needAuth();
    throw new Error('Connexion requise');
  }
  const data = res.status === 204 ? null : await res.json().catch(() => null);
  if (!res.ok) {
    const err = new Error(data?.error || `Erreur ${res.status}`);
    // Certaines réponses en disent plus (alarme : quels capteurs sont ouverts).
    err.status = res.status;
    err.data = data;
    throw err;
  }
  return data;
}

/** 401 : écran de connexion (comptes) ou code d'accès (HEIM_TOKEN sans compte). */
async function needAuth() {
  try {
    const st = await fetch('/api/auth/status').then((r) => r.json());
    if (st.open) home.needToken = true;
    else {
      home.loginUsers = st.users;
      home.needLogin = true;
    }
  } catch {
    home.needLogin = true;
  }
}

export async function loadMe() {
  try {
    home.me = await api('/auth/me');
    home.needLogin = false;
    return true;
  } catch {
    return false;
  }
}

export async function login(name, password) {
  await api('/auth/login', { method: 'POST', body: { name, password } });
  home.needLogin = false;
  await loadMe();
  connect();
}

export async function logout() {
  try {
    await api('/auth/logout', { method: 'POST' });
  } catch {
    /* déjà déconnecté */
  }
  home.me = null;
  if (ws) {
    ws.onclose = null;
    ws.close();
  }
  home.connected = false;
  await needAuth();
}

/** Droits du compte connecté (sans compte : tout est permis, comme avant). */
export function isAdmin() {
  return !home.me || home.me.role === 'admin';
}

export function isMember() {
  return !home.me || home.me.role === 'admin' || home.me.role === 'member';
}

export function setToken(value) {
  token = value.trim();
  storage(KEY_TOKEN, token || null);
  home.needToken = false;
  connect();
}

let ws;
let retry = 1000;
let retryTimer;

/** Premier démarrage : faut-il l'assistant ? (route publique) */
export async function loadSetup() {
  try {
    const st = await fetch('/api/setup').then((r) => r.json());
    home.setup = st.needed ? st : null;
    return st;
  } catch {
    home.setup = null;
    return null;
  }
}

export async function connect() {
  clearTimeout(retryTimer);
  if (home.setup === null && !home.loaded) await loadSetup();
  if (!(await loadMe())) return;
  if (ws) {
    ws.onclose = null;
    ws.close();
  }
  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  const query = token ? '?token=' + encodeURIComponent(token) : '';
  ws = new WebSocket(`${proto}://${location.host}/api/ws${query}`);
  ws.onopen = () => {
    retry = 1000;
    lastMessage = Date.now();
    home.connected = true;
    ws.send(JSON.stringify({ type: 'hello', target_id: deviceId(), name: deviceLabel(), kind: deviceKind() }));
    refreshIntegrations();
    loadBoards();
    loadScreen();
    loadLook();
    loadSun();
    if (isMember()) loadAutomations();
    loadSecurity();
    loadLayout();
    if (isMember()) loadPairing();
  };
  ws.onclose = () => {
    home.connected = false;
    // Un refus d'accès ressemble à une simple coupure : on vérifie avant de réessayer.
    api('/auth/me')
      .then(() => {
        retryTimer = setTimeout(connect, retry);
        retry = Math.min(retry * 2, 15_000);
      })
      .catch(() => {
        if (!home.needToken && !home.needLogin) {
          retryTimer = setTimeout(connect, retry);
          retry = Math.min(retry * 2, 15_000);
        }
      });
  };
  ws.onmessage = (msg) => {
    lastMessage = Date.now();
    handle(JSON.parse(msg.data));
  };
}

// Un Pi débranché ne ferme pas proprement la connexion : sans nouvelles depuis 25 s malgré
// un ping toutes les 10 s, on la considère perdue.
let lastMessage = Date.now();
setInterval(() => {
  if (!ws || ws.readyState !== WebSocket.OPEN) return;
  if (Date.now() - lastMessage > 25_000) {
    ws.onclose = null;
    ws.close();
    home.connected = false;
    clearTimeout(retryTimer);
    retryTimer = setTimeout(connect, 1000);
    return;
  }
  try {
    ws.send(JSON.stringify({ type: 'ping' }));
  } catch {
    /* fermée entre-temps */
  }
}, 10_000);

function clearPending(id) {
  if (id in home.pending) delete home.pending[id];
}

function handle(ev) {
  switch (ev.type) {
    case 'snapshot':
      home.entities = Object.fromEntries(ev.entities.map((e) => [e.entity_id, e]));
      home.loaded = true;
      break;
    case 'entity_updated':
      home.entities[ev.entity.entity_id] = ev.entity;
      break;
    case 'entity_removed':
      delete home.entities[ev.entity_id];
      break;
    case 'state_changed': {
      const e = home.entities[ev.entity_id];
      if (e) e.state = ev.new;
      clearPending(ev.entity_id);
      if (ev.entity_id.startsWith('automation.') && isMember()) loadAutomationsSoon();
      break;
    }
    case 'command_timeout': {
      const name = home.entities[ev.entity_id]?.name || ev.entity_id;
      toast(`${name} ne répond pas. Vérifiez qu'il est alimenté.`, 'alert');
      clearPending(ev.entity_id);
      break;
    }
    case 'integration_status':
      home.integrations[ev.name] = { status: ev.status, detail: ev.detail };
      break;
    case 'notification': {
      const n = ev.notification;
      toast(n.title ? `${n.title} : ${n.message}` : n.message, n.level === 'info' ? 'info' : 'alert');
      break;
    }
    case 'welcome':
      home.targetId = ev.target_id;
      break;
    case 'push_expired':
      break;
    case 'screen_changed':
      home.screensTick++;
      if (ev.id === deviceId()) {
        if (ev.identify) {
          home.identify = true;
          setTimeout(() => (home.identify = false), 6000);
        } else loadScreen();
      }
      break;
    case 'settings_changed':
      if (ev.key === 'dashboards') loadBoards();
      if (ev.key === 'appearance') loadLook();
      if (ev.key === 'location') loadSun();
      if (ev.key === 'accounts') {
        loadMe();
        loadBoards();
        loadLook();
      }
      if (ev.key === 'security' || ev.key === 'security.log') loadSecuritySoon();
      if (ev.key === 'home') loadLayout();
      break;
    case 'security':
      if (home.security) home.security.status = ev.status;
      loadSecuritySoon();
      break;
    case 'zigbee_bridge':
      home.pairing.open = ev.permit_join;
      home.pairing.until = ev.until;
      if (ev.version) home.pairing.version = ev.version;
      break;
    case 'zigbee_device':
      home.pairing.events = [
        { at: new Date().toISOString(), event: ev.event, friendly_name: ev.friendly_name, detail: ev.detail },
        ...home.pairing.events,
      ].slice(0, 20);
      break;
    case 'automation_ran':
      home.lastRun[ev.id] = { ok: ev.ok, detail: ev.detail, at: Date.now() };
      loadAutomations();
      if (!ev.ok && ev.detail) toast(`Automatisme : ${ev.detail}`, 'alert');
      break;
    case 'error':
      if (ev.entity_id) clearPending(ev.entity_id);
      toast(ev.error, 'alert');
      break;
  }
}

export async function refreshIntegrations() {
  try {
    home.integrations = await api('/integrations');
  } catch {
    /* pas grave : les événements suivants rempliront la liste */
  }
}

/** Envoie une commande (« turn_on », « turn_off », « toggle ») et marque l'entité en attente. */
export function command(entityId, action, extra = {}) {
  home.pending[entityId] = Date.now();
  setTimeout(() => {
    if (home.pending[entityId] && Date.now() - home.pending[entityId] >= PENDING_MS) clearPending(entityId);
  }, PENDING_MS);
  if (ws?.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify({ type: 'command', entity_id: entityId, action, ...extra }));
  } else {
    api(`/entities/${entityId}/command`, { method: 'POST', body: { action, ...extra } }).catch((e) => {
      clearPending(entityId);
      toast(e.message, 'alert');
    });
  }
}

/**
 * Modifie un appareil : pièce et masquage pour toutes ses entités, nom pour l'entité affichée.
 * Le nom est toujours renvoyé : une fois l'appareil modifié par l'utilisateur, c'est ce nom-là
 * qui s'affiche, et non plus celui de Zigbee2MQTT.
 */
export async function updateDevice(device, { name, room, hidden }, nameTarget = device.primary) {
  const shown = nameTarget.entity_id === device.primary.entity_id ? device.name : nameTarget.name;
  const finalName = (name ?? shown).trim() || shown;
  const jobs = device.entities.map((e) => {
    const patch = {};
    if (room !== undefined) patch.room = room || null;
    if (hidden !== undefined) patch.hidden = hidden;
    if (e.entity_id === nameTarget.entity_id) patch.name = finalName;
    return api(`/entities/${e.entity_id}`, { method: 'PATCH', body: patch });
  });
  await Promise.all(jobs);
}

/** Allume pour une durée, puis éteint tout seul (minuterie de carte). */
export async function turnOnFor(entityId, minutes) {
  home.pending[entityId] = Date.now();
  try {
    await api(`/entities/${entityId}/turn_on_for`, { method: 'POST', body: { minutes } });
  } catch (e) {
    clearPending(entityId);
    toast(e.message, 'alert');
  }
}

export async function loadBoards() {
  try {
    home.boards = await api('/dashboards');
  } catch {
    /* ancien Heim : rien */
  }
}

/** Le tableau qui s'affiche : imposé (compte, puis écran), sinon le dernier regardé. */
export function currentBoard() {
  const list = home.boards?.boards ?? [];
  if (!list.length) return null;
  const forced = home.boards.locked ? home.boards.home : home.screen?.lock_board ? home.screen.board : null;
  const pick = (id) => list.find((b) => b.id === id);
  return pick(forced) ?? pick(home.board) ?? pick(home.screen?.board) ?? pick(home.boards.home) ?? list[0];
}

/** Les tableaux entre lesquels on peut glisser (ni cachés, ni bloqués). */
export function navBoards() {
  const list = home.boards?.boards ?? [];
  if (home.boards?.locked || home.screen?.lock_board) return [];
  const cur = currentBoard();
  return list.filter((b) => !b.hidden || b.id === cur?.id);
}

export function showBoard(id) {
  home.board = id;
  storage(KEY_BOARD, id);
}

/** Ce compte, sur cet écran, peut-il modifier ou ajouter des cartes ? */
export function boardRights() {
  const b = home.boards;
  const s = home.screen;
  return {
    edit: Boolean(b?.can_edit) && (s?.can_edit ?? true),
    add: Boolean(b?.can_add) && (s?.can_add ?? true),
    manage: Boolean(b?.can_manage) && !s?.kiosk,
  };
}

/** Enregistre les sections d'un tableau (affichées tout de suite, puis confirmées). */
export async function saveBoard(id, sections) {
  const b = home.boards?.boards?.find((x) => x.id === id);
  const before = b ? $state.snapshot(b) : null;
  if (b) {
    b.sections = sections;
    b.auto = false;
  }
  try {
    await api(`/dashboards/${encodeURIComponent(id)}`, { method: 'PUT', body: { sections } });
  } catch (e) {
    if (b && before) Object.assign(b, before);
    toast(e.message, 'alert');
  }
}

/** L'admin enregistre toute la liste (nouveau tableau, renommé, caché, supprimé…). */
export async function saveBoards(boards) {
  try {
    const saved = await api('/dashboards', { method: 'PUT', body: { boards } });
    await loadBoards();
    return saved;
  } catch (e) {
    toast(e.message, 'alert');
    return null;
  }
}

export async function loadScreen() {
  try {
    home.screen = await api(`/screens/${encodeURIComponent(deviceId())}/config`);
  } catch {
    home.screen = null;
  }
}

export async function loadLook() {
  try {
    home.look = await api('/appearance');
  } catch {
    /* ancien Heim */
  }
}

export async function saveMyLook(look) {
  home.look.mine = look;
  try {
    await api('/appearance/me', { method: 'PUT', body: look });
  } catch (e) {
    toast(e.message, 'alert');
  }
}

export async function loadSun() {
  try {
    home.sun = await api('/sun');
  } catch {
    /* rien */
  }
}

/** Onglets cachés à ce compte ou sur cet écran. */
export function hiddenTabs() {
  return [...(home.me?.user?.access?.hidden_tabs ?? []), ...(home.screen?.hidden_tabs ?? [])];
}

export async function loadLayout() {
  try {
    home.layout = await api('/settings/home');
  } catch {
    /* pas de maison décrite */
  }
}

export async function saveLayout(layout) {
  home.layout = await api('/settings/home', { method: 'PUT', body: layout });
  return home.layout;
}

/** Les pièces dans l'ordre de la maison, puis celles trouvées sur les appareils. */
export function homeRooms() {
  const defined = (home.layout?.rooms ?? []).map((r) => r.name);
  const used = roomNames(Object.values(home.entities));
  return [...defined, ...used.filter((u) => !defined.includes(u))];
}

export async function loadSecurity() {
  try {
    home.security = await api('/security');
  } catch {
    /* alarme indisponible : la carte ne s'affiche pas */
  }
}

let securityTimer;
/** Plusieurs événements arrivent d'un coup (état + historique) : un seul rechargement. */
function loadSecuritySoon() {
  clearTimeout(securityTimer);
  securityTimer = setTimeout(loadSecurity, 150);
}

export async function loadPairing() {
  try {
    home.pairing = await api('/zigbee/pairing');
  } catch {
    /* pas de Zigbee */
  }
}

/** Adresse d'une photo de caméra (le jeton passe dans l'URL, une image ne sait pas l'envoyer). */
export function snapshotUrl(name) {
  const q = token ? '?token=' + encodeURIComponent(token) : '';
  return `/api/security/snapshots/${encodeURIComponent(name)}${q}`;
}

let automationsTimer;
function loadAutomationsSoon() {
  clearTimeout(automationsTimer);
  automationsTimer = setTimeout(loadAutomations, 200);
}

export async function loadAutomations() {
  try {
    home.automations = await api('/automations');
  } catch {
    /* rien */
  }
}

/**
 * Les infos techniques (signal, « vu il y a ») changent sans cesse : le noyau ne les pousse pas
 * en direct pour ne pas inonder le réseau. On les relit de temps en temps.
 */
export async function refreshEntities() {
  try {
    const list = await api('/entities');
    for (const e of list) {
      const current = home.entities[e.entity_id];
      if (current && e.state) current.state = { ...current.state, ...e.state };
      else if (!current) home.entities[e.entity_id] = e;
    }
  } catch {
    /* réessai au prochain tour */
  }
}

setInterval(() => {
  home.now = Date.now();
}, 500);

// Téléphone déverrouillé, appli revenue au premier plan : on se reconnecte tout de suite
// au lieu d'attendre la prochaine tentative.
if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible' && !home.connected && home.loaded) {
      retry = 1000;
      connect();
    }
  });
}

setInterval(() => {
  if (home.connected) refreshEntities();
}, 60_000);
