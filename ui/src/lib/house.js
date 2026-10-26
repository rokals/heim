// Transforme la liste brute d'entités en une maison lisible :
// des appareils (une ampoule = une carte, même si elle publie 5 valeurs), rangés par pièce.
// Fonctions pures : testées avec `npm test`.

export const UNSORTED = '';

const CONTROL = new Set(['light', 'switch']);

// Ce qu'on lit en premier sur un capteur qui mesure plusieurs choses.
const READING_ORDER = ['contact', 'water_leak', 'smoke', 'occupancy', 'presence', 'temperature', 'humidity', 'co2', 'power', 'illuminance', 'illuminance_lux', 'pressure'];
const rank = (e) => {
  const i = READING_ORDER.indexOf(e.capabilities?.kind);
  return i === -1 ? READING_ORDER.length : i;
};
const READING = new Set(['sensor', 'binary_sensor']);
// Une sirène est un appareil à part : visible dans Appareils, jamais une tuile d'accueil
// (on ne veut pas la déclencher en effleurant l'écran).
const SIREN = 'siren';

export function isOn(entity) {
  const v = entity?.state?.value;
  return v === 'on' || v === 'running';
}

export function isAvailable(entity) {
  return entity?.state?.available !== false && entity?.state != null;
}

function deviceKey(e) {
  return e.device_id || e.entity_id;
}

/** Nom affiché d'un appareil : le nom choisi par l'utilisateur, sinon celui de Zigbee2MQTT. */
export function deviceName(device) {
  const p = device.primary;
  if (p.user_edited) return p.name;
  return p.capabilities?.device_name || p.name;
}

function pickPrimary(list) {
  const readings = list
    .filter((e) => READING.has(e.domain) && e.category !== 'diagnostic')
    .sort((a, b) => rank(a) - rank(b));
  return (
    list.find((e) => CONTROL.has(e.domain) && e.category !== 'diagnostic') ||
    list.find((e) => e.domain === SIREN) ||
    readings[0] ||
    list[0]
  );
}

/** Regroupe les entités physiques par appareil. Scènes et timers sont à part. */
export function groupDevices(entities) {
  const byKey = new Map();
  for (const e of entities) {
    if (!CONTROL.has(e.domain) && !READING.has(e.domain) && e.domain !== SIREN) continue;
    const key = deviceKey(e);
    if (!byKey.has(key)) byKey.set(key, []);
    byKey.get(key).push(e);
  }
  const devices = [];
  for (const [key, list] of byKey) {
    list.sort((a, b) => a.entity_id.localeCompare(b.entity_id));
    const primary = pickPrimary(list);
    const battery = list.find((e) => e.capabilities?.kind === 'battery');
    const batteryLevel = typeof battery?.state?.value === 'number' ? battery.state.value : null;
    const device = {
      key,
      primary,
      entities: list,
      controls: list.filter((e) => CONTROL.has(e.domain) && e.category !== 'diagnostic'),
      sirens: list.filter((e) => e.domain === SIREN),
      readings: list
        .filter((e) => READING.has(e.domain) && e.category !== 'diagnostic')
        .sort((a, b) => rank(a) - rank(b)),
      diagnostics: list.filter((e) => e.category === 'diagnostic'),
      room: list.find((e) => e.room)?.room || UNSORTED,
      hidden: list.every((e) => e.hidden),
      battery: batteryLevel,
    };
    device.name = deviceName(device);
    devices.push(device);
  }
  return devices.sort((a, b) => a.name.localeCompare(b.name, 'fr'));
}

/** Les pièces, avec leurs commandes (lumières, prises) et leurs relevés (capteurs). */
export function buildRooms(entities, order = []) {
  const devices = groupDevices(entities).filter((d) => !d.hidden);
  const rooms = new Map();
  const room = (name) => {
    if (!rooms.has(name)) rooms.set(name, { name, controls: [], sensors: [], timers: [] });
    return rooms.get(name);
  };
  for (const d of devices) {
    const r = room(d.room);
    if (d.controls.length) {
      for (const c of d.controls) r.controls.push({ entity: c, device: d });
    } else if (d.readings.length) {
      r.sensors.push(d);
    }
  }
  for (const e of entities) {
    if (e.domain === 'timer' && !e.hidden) room(e.room || UNSORTED).timers.push(e);
    // Un groupe (« tout le salon ») passe en premier dans sa pièce.
    if (e.domain === 'group' && !e.hidden) room(e.room || UNSORTED).controls.unshift({ entity: e, device: null });
  }
  // L'ordre de la maison (Réglages > Maison) d'abord, puis l'ordre alphabétique.
  const pos = (n) => {
    const i = order.indexOf(n);
    return i === -1 ? order.length : i;
  };
  return [...rooms.values()].sort((a, b) => {
    if (a.name === UNSORTED) return 1;
    if (b.name === UNSORTED) return -1;
    return pos(a.name) - pos(b.name) || a.name.localeCompare(b.name, 'fr');
  });
}

export function roomNames(entities) {
  const names = new Set();
  for (const e of entities) if (e.room) names.add(e.room);
  return [...names].sort((a, b) => a.localeCompare(b, 'fr'));
}

/** « 3 lumières allumées », pour l'en-tête. */
export function houseSummary(entities) {
  const lights = entities.filter((e) => e.domain === 'light' && !e.hidden && e.category !== 'diagnostic');
  const plugs = entities.filter((e) => e.domain === 'switch' && !e.hidden && e.category !== 'diagnostic');
  const lightsOn = lights.filter(isOn);
  const plugsOn = plugs.filter(isOn);
  return { lights, plugs, lightsOn, plugsOn };
}

export function slug(text) {
  return text
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '');
}

// ---- Infos techniques : signal, « vu il y a », minuterie de carte -------------------

/** Qualité du signal Zigbee (LQI, 0 à 255) la plus récente parmi les entités de l'appareil. */
export function signal(device) {
  for (const e of device.entities) {
    const lq = e.state?.attributes?.linkquality;
    if (typeof lq === 'number') return lq;
  }
  return null;
}

/** LQI -> 0 à 4 barres. */
export function signalBars(lqi) {
  if (lqi == null) return null;
  if (lqi >= 150) return 4;
  if (lqi >= 100) return 3;
  if (lqi >= 50) return 2;
  if (lqi > 20) return 1;
  return 0;
}

/** Dernier message reçu de l'appareil (n'importe laquelle de ses entités). */
export function lastSeen(device) {
  let best = null;
  for (const e of device.entities) {
    const t = e.state?.last_seen || null;
    if (t && (!best || t > best)) best = t;
  }
  return best;
}

/** « à l'instant », « il y a 5 min », « il y a 3 h », « il y a 2 j » */
export function ago(iso, now = Date.now()) {
  if (!iso) return 'jamais vu';
  const s = Math.max(0, (now - new Date(iso).getTime()) / 1000);
  if (s < 60) return "à l'instant";
  if (s < 3600) return `il y a ${Math.floor(s / 60)} min`;
  if (s < 86400) return `il y a ${Math.floor(s / 3600)} h`;
  return `il y a ${Math.floor(s / 86400)} j`;
}

/** Identifiant de la minuterie automatique d'une entité (même règle que le noyau). */
export function autoTimerId(entityId) {
  return `timer.auto_${slug(entityId)}`;
}

/** Secondes restantes de la minuterie de carte, ou null. */
export function autoRemaining(entityId, entities, now) {
  const t = entities[autoTimerId(entityId)];
  if (t?.state?.value !== 'running') return null;
  const end = t.state.attributes?.ends_at;
  return end ? Math.max(0, (new Date(end).getTime() - now) / 1000) : null;
}

// ---- Tableau de bord -------------------------------------------------------------

/** entity_id -> appareil, pour retrouver une carte à partir de ce qui est enregistré. */
export function deviceIndex(entities) {
  const map = new Map();
  for (const d of groupDevices(entities)) for (const e of d.entities) map.set(e.entity_id, d);
  return map;
}

/** Le rangement automatique, converti en tableau de bord modifiable. */
export function dashboardFromRooms(entities, order = []) {
  return {
    version: 2,
    sections: buildRooms(entities, order)
      .filter((r) => r.controls.length || r.sensors.length)
      .map((r) => ({
        title: r.name === UNSORTED ? 'À ranger' : r.name,
        cards: [
          ...r.controls.map((c) => ({ entity_id: c.entity.entity_id, w: 2, h: 2 })),
          ...r.sensors.map((d) => ({ entity_id: d.primary.entity_id, w: 4, h: 1 })),
        ],
      })),
  };
}

/** Tout ce qui pourrait aller sur l'accueil et n'y est pas encore. */
export function missingCards(dashboard, entities) {
  const used = new Set((dashboard?.sections || []).flatMap((s) => s.cards.map((c) => c.entity_id)));
  const out = [];
  for (const d of groupDevices(entities).filter((d) => !d.hidden)) {
    if (d.controls.length) {
      for (const c of d.controls) if (!used.has(c.entity_id)) out.push({ entity: c, device: d });
    } else if (!d.entities.some((e) => used.has(e.entity_id))) {
      out.push({ entity: d.primary, device: d });
    }
  }
  for (const e of entities) {
    if (e.domain === 'scene' && !used.has(e.entity_id)) out.push({ entity: e, device: null });
  }
  return out;
}
