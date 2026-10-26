import { houseRules } from './housecards.js';

// Les boutons du tableau de bord : ce qu'ils montrent, ce que font l'appui court et l'appui
// long, et leurs règles d'apparence (« si température > 25 : orange, "Chaud" »).
// Fonctions pures, testées avec `npm test`.

/** Couleurs proposées pour les règles (nom → teinte). */
export const COLORS = {
  ember: '#f2a541',
  red: '#e5484d',
  orange: '#f76b15',
  yellow: '#ffd23f',
  green: '#46a758',
  teal: '#12a594',
  blue: '#3e63dd',
  sky: '#0090ff',
  purple: '#8e4ec6',
  pink: '#d6409f',
  gray: '#8b8d98',
};

export const COLOR_NAMES = {
  ember: 'Braise',
  red: 'Rouge',
  orange: 'Orange',
  yellow: 'Jaune',
  green: 'Vert',
  teal: 'Turquoise',
  blue: 'Bleu',
  sky: 'Ciel',
  purple: 'Violet',
  pink: 'Rose',
  gray: 'Gris',
};

/** L'alarme en couleur sur sa carte. */
const TONE_COLORS = { armed: COLORS.green, arming: COLORS.orange, pending: COLORS.orange, alarm: COLORS.red };

/** Icônes qu'on peut choisir pour un bouton. */
export const TILE_ICONS = [
  'bulb', 'plug', 'scene', 'auto', 'mail', 'bell', 'shield', 'door', 'window', 'drop', 'flame',
  'thermo', 'sun', 'moon', 'fan', 'plant', 'car', 'trash', 'washer', 'paw', 'heart', 'star',
  'check', 'alert', 'motion', 'sofa', 'bed', 'kitchen', 'bath', 'tree', 'battery', 'power',
];

export const OPS = [
  { id: 'eq', label: 'est' },
  { id: 'ne', label: "n'est pas" },
  { id: 'gt', label: '>' },
  { id: 'lt', label: '<' },
  { id: 'gte', label: '≥' },
  { id: 'lte', label: '≤' },
];

/** De quoi parle-t-on : un appareil, une scène, un automatisme, un groupe… */
export function domainOf(entityId) {
  return String(entityId || '').split('.')[0];
}

/**
 * La valeur que montre un bouton (et que testent ses règles).
 * Automatisme : son état (vrai/faux), ou s'il est activé (facette « enabled »).
 */
export function tileValue(card, entity) {
  if (!entity?.state) return null;
  const d = domainOf(entity.entity_id);
  if (d === 'automation') {
    if (card?.facet === 'enabled') return entity.state.attributes?.enabled !== false;
    return entity.state.value === 'on';
  }
  if (d === 'scene' || d === 'group') return entity.state.value === 'on';
  return entity.state.value;
}

/** Les cartes Maison (et d'autres plus tard) portent leur texte tout prêt. */
function ownText(entity) {
  return domainOf(entity?.entity_id) === 'house' ? (entity.state?.attributes?.text ?? '') : null;
}

/** Les mots d'une valeur : « vrai », « allumé », « ouvert », « 21,5 °C »… */
export function valueText(card, entity) {
  const own = ownText(entity);
  if (own != null) return own;
  const v = tileValue(card, entity);
  if (v == null) return entity?.state?.available === false ? 'Injoignable' : '–';
  const d = domainOf(entity.entity_id);
  if (d === 'automation') {
    if (card?.facet === 'enabled') return v ? 'Activé' : 'Désactivé';
    return v ? 'Vrai' : 'Faux';
  }
  if (d === 'scene') return v ? (entity.state.attributes?.partial ? 'En partie' : 'Active') : 'Inactive';
  if (typeof v === 'boolean') return v ? 'Allumé' : 'Éteint';
  if (d === 'binary_sensor') {
    const kind = entity.capabilities?.kind;
    const on = v === 'on';
    if (kind === 'contact') return on ? 'Ouvert' : 'Fermé';
    if (kind === 'water_leak') return on ? 'Fuite !' : 'Sec';
    if (kind === 'smoke') return on ? 'Fumée !' : 'Rien';
    if (kind === 'occupancy' || kind === 'presence') return on ? 'Présence' : 'Personne';
    return on ? 'Actif' : 'Inactif';
  }
  if (d === 'person') return v === 'home' ? 'À la maison' : 'Absent';
  if (v === 'on') return 'Allumé';
  if (v === 'off') return 'Éteint';
  if (typeof v === 'number') {
    const unit = entity.capabilities?.unit;
    const s = Number.isInteger(v) ? String(v) : v.toFixed(1).replace('.', ',');
    return unit ? `${s} ${unit}` : s;
  }
  return String(v);
}

/** Une valeur saisie dans une règle (« 25 », « vrai », « on ») comparée à la vraie. */
function normalize(x) {
  if (typeof x === 'boolean' || typeof x === 'number') return x;
  const s = String(x ?? '').trim().toLowerCase();
  if (['vrai', 'true', 'oui', 'allumé', 'allume', 'ouvert', 'actif', 'active', 'on'].includes(s)) return 'on';
  if (['faux', 'false', 'non', 'éteint', 'eteint', 'fermé', 'ferme', 'inactif', 'inactive', 'off'].includes(s)) return 'off';
  const n = Number(s.replace(',', '.'));
  return s !== '' && Number.isFinite(n) ? n : s;
}

function asComparable(v) {
  if (v === true) return 'on';
  if (v === false) return 'off';
  return normalize(v);
}

export function ruleMatches(rule, value) {
  if (value == null) return false;
  const a = asComparable(value);
  const b = normalize(rule.value);
  switch (rule.op) {
    case 'eq':
      return a === b;
    case 'ne':
      return a !== b;
    case 'gt':
      return typeof a === 'number' && typeof b === 'number' && a > b;
    case 'lt':
      return typeof a === 'number' && typeof b === 'number' && a < b;
    case 'gte':
      return typeof a === 'number' && typeof b === 'number' && a >= b;
    case 'lte':
      return typeof a === 'number' && typeof b === 'number' && a <= b;
    default:
      return false;
  }
}

/** Règles d'office quand le bouton n'en a pas : fuite en rouge, chaud en orange… */
export function defaultRules(card, entity) {
  const d = domainOf(entity?.entity_id);
  if (d === 'house') return houseRules(card);
  const kind = entity?.capabilities?.kind;
  if (d === 'automation' && card?.facet !== 'enabled') return [{ op: 'eq', value: 'vrai', color: 'red' }];
  if (d === 'binary_sensor') {
    if (kind === 'water_leak' || kind === 'smoke') return [{ op: 'eq', value: 'on', color: 'red' }];
    if (kind === 'contact') return [{ op: 'eq', value: 'on', color: 'orange' }];
    if (kind === 'occupancy' || kind === 'presence') return [{ op: 'eq', value: 'on', color: 'yellow' }];
  }
  if (kind === 'temperature')
    return [
      { op: 'gte', value: 26, color: 'orange' },
      { op: 'lte', value: 16, color: 'sky' },
    ];
  if (kind === 'humidity') return [{ op: 'gte', value: 70, color: 'blue' }];
  if (kind === 'co2') return [{ op: 'gte', value: 1200, color: 'red' }, { op: 'gte', value: 900, color: 'orange' }];
  if (kind === 'battery') return [{ op: 'lte', value: 15, color: 'red' }];
  return [];
}

/** « Courrier depuis {depuis} » → « Courrier depuis 2 h ». */
export function fillText(text, { value, since, name }) {
  return String(text)
    .replaceAll('{valeur}', value ?? '')
    .replaceAll('{depuis}', since ?? '')
    .replaceAll('{nom}', name ?? '');
}

/** Depuis quand : « 5 min », « 2 h », « 3 j ». */
export function sinceText(iso, now = Date.now()) {
  if (!iso) return '';
  const s = Math.max(0, (now - new Date(iso).getTime()) / 1000);
  if (s < 60) return "moins d'une minute";
  if (s < 3600) return `${Math.floor(s / 60)} min`;
  if (s < 86400) return `${Math.floor(s / 3600)} h`;
  return `${Math.floor(s / 86400)} j`;
}

/**
 * Tout ce qu'il faut pour dessiner un bouton : couleur (ou null), icône, texte, actif.
 * La première règle vraie gagne ; sans règle à soi, les règles d'office.
 */
export function appearance(card, entity, now = Date.now()) {
  const value = tileValue(card, entity);
  const text = valueText(card, entity);
  const rules = card?.rules?.length ? card.rules : defaultRules(card, entity);
  const rule = rules.find((r) => ruleMatches(r, value)) ?? null;
  const since = sinceText(
    domainOf(entity?.entity_id) === 'automation' && card?.facet !== 'enabled'
      ? entity?.state?.attributes?.fired_at || entity?.state?.last_changed
      : entity?.state?.last_changed,
    now,
  );
  const vars = { value: text, since, name: card?.name || entity?.name };
  // Une scène active prend sa couleur à elle, quand elle en a une.
  const house = domainOf(entity?.entity_id) === 'house';
  const tone = house ? entity.state?.attributes?.tone : null;
  const own = rule
    ? null
    : domainOf(entity?.entity_id) === 'scene' && value === true
      ? entity?.capabilities?.color
      : tone
        ? TONE_COLORS[tone] ?? null
        : null;
  return {
    color: rule?.color ? (COLORS[rule.color] ?? rule.color) : (own ?? null),
    icon: rule?.icon || card?.icon || defaultIcon(entity),
    text: rule?.text ? fillText(rule.text, vars) : text,
    rule,
    value,
    active: house ? Boolean(entity.state?.attributes?.active) : value === true || value === 'on' || value === 'home',
  };
}

export function defaultIcon(entity) {
  const d = domainOf(entity?.entity_id);
  if (d === 'house') return entity?.capabilities?.icon || 'house';
  const kind = entity?.capabilities?.kind;
  if (d === 'light') return 'bulb';
  if (d === 'switch') return 'plug';
  if (d === 'group') return 'bulb';
  if (d === 'scene') return entity?.capabilities?.icon || 'scene';
  if (d === 'automation') return 'auto';
  if (d === 'person') return 'user';
  if (d === 'siren') return 'bell';
  if (kind === 'temperature') return 'thermo';
  if (kind === 'humidity' || kind === 'water_leak') return 'drop';
  if (kind === 'contact') return 'door';
  if (kind === 'occupancy' || kind === 'presence') return 'motion';
  if (kind === 'smoke') return 'flame';
  if (kind === 'battery') return 'battery';
  if (kind === 'power') return 'power';
  return 'sensor';
}

/** Ce que peut faire chaque type de chose : la liste des actions proposées. */
export function actionsFor(entityId, facet) {
  const d = domainOf(entityId);
  if (d === 'automation') {
    if (facet === 'enabled')
      return ['toggle', 'on', 'off', 'run', 'details', 'none'];
    return ['rearm', 'toggle', 'on', 'off', 'run', 'details', 'none'];
  }
  if (['light', 'switch', 'group', 'scene', 'siren'].includes(d)) return ['toggle', 'on', 'off', 'details', 'none'];
  return ['details', 'none'];
}

export const ACTION_LABELS = {
  toggle: 'Basculer',
  on: 'Allumer / activer',
  off: 'Éteindre / désactiver',
  rearm: 'Réarmer',
  run: 'Lancer maintenant',
  details: 'Ouvrir le détail',
  none: 'Rien',
};

/** Libellés adaptés : « Mettre à vrai » pour un état, « Activer » pour un automatisme… */
export function actionLabel(action, entityId, facet) {
  const d = domainOf(entityId);
  if (d === 'automation' && facet !== 'enabled') {
    if (action === 'toggle') return 'Inverser l’état';
    if (action === 'on') return 'Mettre à vrai';
    if (action === 'off') return 'Mettre à faux';
  }
  if (d === 'automation' && facet === 'enabled') {
    if (action === 'toggle') return 'Activer / désactiver';
    if (action === 'on') return 'Activer';
    if (action === 'off') return 'Désactiver';
  }
  if (d === 'scene') {
    if (action === 'on') return 'Activer';
    if (action === 'off') return 'Désactiver';
  }
  return ACTION_LABELS[action] ?? action;
}

/** Réglages de départ d'un bouton selon ce qu'on y met. */
export function defaultCard(entity, choice = {}) {
  const id = entity.entity_id;
  const d = domainOf(id);
  const card = { entity_id: id, w: 2, h: 2 };
  if (d === 'automation') {
    card.facet = choice.facet ?? 'state';
    card.tap = choice.tap ?? (card.facet === 'state' ? 'details' : 'toggle');
    card.hold = card.facet === 'state' ? 'rearm' : 'details';
    return card;
  }
  if (['light', 'switch', 'group', 'scene'].includes(d)) {
    card.tap = choice.tap ?? 'toggle';
    card.hold = 'details';
    return card;
  }
  card.tap = 'details';
  card.hold = 'details';
  if (d === 'sensor' || d === 'binary_sensor') Object.assign(card, { w: 4, h: 1 });
  return card;
}

/** Ce qu'il faut envoyer pour une action : { kind: 'command'|'automation'|'details'|'none', … } */
export function resolveAction(action, card, entity) {
  const id = entity?.entity_id;
  const d = domainOf(id);
  if (!action || action === 'none') return { kind: 'none' };
  if (action === 'details') return { kind: 'details' };
  if (d === 'automation') {
    const automation = id.slice('automation.'.length);
    const enabled = entity?.state?.attributes?.enabled !== false;
    const state = entity?.state?.value === 'on';
    if (action === 'run') return { kind: 'automation', automation, body: { run: true } };
    if (action === 'rearm') return { kind: 'automation', automation, body: { state: 'rearm' } };
    if (card?.facet === 'enabled') {
      const next = action === 'toggle' ? !enabled : action === 'on';
      return { kind: 'automation', automation, body: { enabled: next } };
    }
    const next = action === 'toggle' ? !state : action === 'on';
    return { kind: 'automation', automation, body: { state: next } };
  }
  if (action === 'toggle') return { kind: 'command', entity_id: id, action: 'toggle' };
  if (action === 'on') return { kind: 'command', entity_id: id, action: 'turn_on' };
  if (action === 'off') return { kind: 'command', entity_id: id, action: 'turn_off' };
  return { kind: 'none' };
}

/** Ce que propose la liste « Ajouter » pour une chose : ses façons d'aller sur l'accueil. */
export function choicesFor(entity) {
  const d = domainOf(entity.entity_id);
  if (d === 'automation')
    return [
      { label: 'État', hint: 'Vrai / faux, appui long pour réarmer', facet: 'state', tap: 'details' },
      { label: 'Basculer', hint: 'Activer ou désactiver d’un appui', facet: 'enabled', tap: 'toggle' },
      { label: 'Activer', hint: 'Un appui l’active', facet: 'enabled', tap: 'on' },
      { label: 'Désactiver', hint: 'Un appui le désactive', facet: 'enabled', tap: 'off' },
    ];
  if (['light', 'switch', 'group', 'scene'].includes(d))
    return [
      { label: 'Basculer', hint: 'Allume ou éteint', tap: 'toggle' },
      { label: d === 'scene' ? 'Activer' : 'Allumer', hint: 'Un appui allume', tap: 'on' },
      { label: d === 'scene' ? 'Désactiver' : 'Éteindre', hint: 'Un appui éteint', tap: 'off' },
      { label: 'État', hint: 'Affiche seulement', tap: 'details' },
    ];
  return [{ label: 'État', hint: 'Affiche la valeur', tap: 'details' }];
}
