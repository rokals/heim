// Automatismes : vocabulaire, valeurs proposées selon l'appareil, modèles prêts à l'emploi,
// et une phrase lisible pour la liste.

import { kindOf } from './format.js';

const BINARY_WORDS = {
  contact: ['Ouverte', 'Fermée'],
  occupancy: ['Présence', 'Personne'],
  presence: ['Présence', 'Personne'],
  water_leak: ['Fuite', 'Au sec'],
  smoke: ['Fumée', 'Normal'],
  vibration: ['Vibration', 'Calme'],
};

/** Les états qu'on peut attendre d'un appareil, avec leur nom en français. */
export function stateChoices(entity) {
  if (!entity) return [];
  if (entity.domain === 'automation') {
    return [
      { value: 'on', label: 'vrai' },
      { value: 'off', label: 'faux' },
    ];
  }
  if (entity.domain === 'light' || entity.domain === 'switch' || entity.domain === 'scene' || entity.domain === 'group') {
    return [
      { value: 'on', label: entity.domain === 'scene' ? 'active' : 'allumé' },
      { value: 'off', label: entity.domain === 'scene' ? 'inactive' : 'éteint' },
    ];
  }
  if (entity.domain === 'person') {
    return [
      { value: 'home', label: 'à la maison' },
      { value: 'away', label: 'absent' },
    ];
  }
  if (entity.domain === 'binary_sensor') {
    const [on, off] = BINARY_WORDS[kindOf(entity)] || ['actif', 'inactif'];
    return [
      { value: 'on', label: on.toLowerCase() },
      { value: 'off', label: off.toLowerCase() },
    ];
  }
  return [];
}

export function stateLabel(entity, value) {
  return stateChoices(entity).find((c) => c.value === value)?.label ?? String(value);
}

export const DAYS = ['lun', 'mar', 'mer', 'jeu', 'ven', 'sam', 'dim'];

export const TRIGGER_TYPES = [
  { type: 'state', label: 'Un appareil change' },
  { type: 'numeric', label: 'Une mesure franchit un seuil' },
  { type: 'time', label: 'À une heure précise' },
  { type: 'sun', label: 'Au lever ou au coucher du soleil' },
];

export const CONDITION_TYPES = [
  { type: 'time', label: 'Entre deux heures' },
  { type: 'dark', label: 'Il fait nuit' },
  { type: 'daylight', label: 'Il fait jour' },
  { type: 'days', label: 'Certains jours' },
  { type: 'state', label: 'Un appareil est dans un état' },
];

export const STEP_TYPES = [
  { type: 'command', label: 'Allumer, éteindre ou basculer' },
  { type: 'turn_on_for', label: 'Allumer pour un temps' },
  { type: 'scene', label: 'Activer une scène' },
  { type: 'notify', label: 'Envoyer une notification' },
  { type: 'delay', label: 'Attendre' },
  { type: 'automation', label: 'Réarmer, activer ou lancer un automatisme' },
];

export function blankTrigger(type) {
  switch (type) {
    case 'state':
      return { type, entity_id: '', to: 'on' };
    case 'numeric':
      return { type, entity_id: '', above: 25 };
    case 'time':
      return { type, at: '07:00', days: [] };
    default:
      return { type: 'sun', event: 'sunset', offset_min: 0 };
  }
}

export function blankCondition(type) {
  switch (type) {
    case 'time':
      return { type, after: '22:00', before: '06:00' };
    case 'days':
      return { type, days: [1, 2, 3, 4, 5] };
    case 'state':
      return { type, entity_id: '', is: 'on' };
    default:
      return { type };
  }
}

export function blankStep(type) {
  switch (type) {
    case 'command':
      return { type, entity_id: '', action: 'turn_on' };
    case 'turn_on_for':
      return { type, entity_id: '', minutes: 5 };
    case 'scene':
      return { type, scene_id: '', on: true };
    case 'notify':
      return { type, message: '' };
    case 'automation':
      return { type, automation_id: '', op: 'rearm' };
    default:
      return { type: 'delay', seconds: 60 };
  }
}

/** Modèles pour démarrer sans réfléchir ; l'utilisateur n'a qu'à choisir ses appareils. */
export const PRESETS = [
  {
    title: "Éclairer l'entrée la nuit",
    detail: "Quand la porte s'ouvre et qu'il fait nuit, allumer pour 5 minutes.",
    value: {
      name: "Entrée éclairée la nuit",
      triggers: [{ type: 'state', entity_id: '', to: 'on' }],
      conditions: [{ type: 'dark' }],
      actions: [{ type: 'turn_on_for', entity_id: '', minutes: 5 }],
    },
  },
  {
    title: 'Porte restée ouverte',
    detail: 'Prévenir si une porte reste ouverte plus de 10 minutes.',
    value: {
      name: 'Porte restée ouverte',
      triggers: [{ type: 'state', entity_id: '', to: 'on', for_s: 600 }],
      conditions: [],
      actions: [{ type: 'notify', message: 'La porte est ouverte depuis 10 minutes.' }],
    },
  },
  {
    title: 'Bienvenue à la maison',
    detail: "Quand quelqu'un rentre et qu'il fait nuit, allumer l'entrée 5 minutes (présence par le Wi-Fi).",
    value: {
      name: 'Bienvenue à la maison',
      triggers: [{ type: 'state', entity_id: '', to: 'home' }],
      conditions: [{ type: 'dark' }],
      actions: [{ type: 'turn_on_for', entity_id: '', minutes: 5 }],
    },
  },
  {
    title: 'Courrier arrivé',
    detail: "Quand la boîte aux lettres s'ouvre : prévenir et lever l'état « Courrier ». Réarmé quand on relève le courrier (trappe arrière) ou à la main.",
    value: {
      name: 'Courrier',
      triggers: [{ type: 'state', entity_id: '', to: 'on' }],
      conditions: [],
      actions: [{ type: 'notify', message: 'Le facteur est passé.' }],
      rest_state: false,
      rearm: { triggers: [{ type: 'state', entity_id: '', to: 'on' }], conditions: [] },
    },
  },
  {
    title: 'Tout éteindre le soir',
    detail: 'À 23 h 30, éteindre les lumières choisies.',
    value: {
      name: 'Extinction du soir',
      triggers: [{ type: 'time', at: '23:30', days: [] }],
      conditions: [],
      actions: [{ type: 'command', entity_id: '', action: 'turn_off' }],
    },
  },
];

/** « Réarmé quand … », pour la liste. */
export function describeRearm(rearm, entities) {
  const parts = (rearm?.triggers ?? []).map((t) => describeTrigger(t, entities));
  if (rearm?.after_min) parts.push(`au bout de ${rearm.after_min >= 60 && rearm.after_min % 60 === 0 ? `${rearm.after_min / 60} h` : `${rearm.after_min} min`}`);
  return parts.join(' ou ');
}

/** « Quand Porte d'entrée devient ouverte » — pour la liste. */
export function describeTrigger(t, entities) {
  const name = (id) => entities[id]?.name ?? (id || '…');
  switch (t.type) {
    case 'state': {
      const base = `${name(t.entity_id)} devient ${stateLabel(entities[t.entity_id], t.to)}`;
      return t.for_s ? `${base} depuis ${Math.round(t.for_s / 60)} min` : base;
    }
    case 'numeric': {
      const parts = [];
      if (t.above != null) parts.push(`dépasse ${t.above}`);
      if (t.below != null) parts.push(`passe sous ${t.below}`);
      return `${name(t.entity_id)} ${parts.join(' et ')}`;
    }
    case 'time': {
      const days = t.days?.length ? ` (${t.days.map((d) => DAYS[d - 1]).join(', ')})` : '';
      return `à ${t.at}${days}`;
    }
    case 'sun': {
      const what = t.event === 'sunrise' ? 'au lever du soleil' : 'au coucher du soleil';
      if (!t.offset_min) return what;
      return `${Math.abs(t.offset_min)} min ${t.offset_min < 0 ? 'avant' : 'après'} ${what.slice(3)}`;
    }
  }
  return '';
}
