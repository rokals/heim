// Automatismes : vocabulaire, valeurs proposées selon l'appareil, modèles prêts à l'emploi,
// et une phrase lisible pour la liste.

import { tr } from './i18n.js';
import { kindOf } from './format.js';

const BINARY_WORDS = {
  contact: [tr("Ouverte"), tr("Fermée")],
  occupancy: [tr("Présence"), tr("Personne")],
  presence: [tr("Présence"), tr("Personne")],
  water_leak: [tr("Fuite"), tr("Au sec")],
  smoke: [tr("Fumée"), tr("Normal")],
  vibration: [tr("Vibration"), tr("Calme")],
};

/** Les états qu'on peut attendre d'un appareil, avec leur nom en français. */
export function stateChoices(entity) {
  if (!entity) return [];
  if (entity.domain === 'automation') {
    return [
      { value: 'on', label: tr("vrai") },
      { value: 'off', label: tr("faux") },
    ];
  }
  if (entity.domain === 'light' || entity.domain === 'switch' || entity.domain === 'scene' || entity.domain === 'group') {
    return [
      { value: 'on', label: entity.domain === 'scene' ? 'active' : tr("allumé") },
      { value: 'off', label: entity.domain === 'scene' ? 'inactive' : tr("éteint") },
    ];
  }
  if (entity.domain === 'person') {
    return [
      { value: 'home', label: tr("à la maison") },
      { value: 'away', label: tr("absent") },
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
  { type: 'state', label: tr("Un appareil change") },
  { type: 'numeric', label: tr("Une mesure franchit un seuil") },
  { type: 'time', label: tr("À une heure précise") },
  { type: 'sun', label: tr("Au lever ou au coucher du soleil") },
];

export const CONDITION_TYPES = [
  { type: 'time', label: tr("Entre deux heures") },
  { type: 'dark', label: tr("Il fait nuit") },
  { type: 'daylight', label: tr("Il fait jour") },
  { type: 'days', label: tr("Certains jours") },
  { type: 'state', label: tr("Un appareil est dans un état") },
];

export const STEP_TYPES = [
  { type: 'command', label: tr("Allumer, éteindre ou basculer") },
  { type: 'turn_on_for', label: tr("Allumer pour un temps") },
  { type: 'scene', label: tr("Activer une scène") },
  { type: 'notify', label: tr("Envoyer une notification") },
  { type: 'delay', label: tr("Attendre") },
  { type: 'automation', label: tr("Réarmer, activer ou lancer un automatisme") },
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
    title: tr("Éclairer l'entrée la nuit"),
    detail: tr("Quand la porte s'ouvre et qu'il fait nuit, allumer pour 5 minutes."),
    value: {
      name: tr("Entrée éclairée la nuit"),
      triggers: [{ type: 'state', entity_id: '', to: 'on' }],
      conditions: [{ type: 'dark' }],
      actions: [{ type: 'turn_on_for', entity_id: '', minutes: 5 }],
    },
  },
  {
    title: tr("Porte restée ouverte"),
    detail: tr("Prévenir si une porte reste ouverte plus de 10 minutes."),
    value: {
      name: tr("Porte restée ouverte"),
      triggers: [{ type: 'state', entity_id: '', to: 'on', for_s: 600 }],
      conditions: [],
      actions: [{ type: 'notify', message: tr("La porte est ouverte depuis 10 minutes.") }],
    },
  },
  {
    title: tr("Bienvenue à la maison"),
    detail: tr("Quand quelqu'un rentre et qu'il fait nuit, allumer l'entrée 5 minutes (présence par le Wi-Fi)."),
    value: {
      name: tr("Bienvenue à la maison"),
      triggers: [{ type: 'state', entity_id: '', to: 'home' }],
      conditions: [{ type: 'dark' }],
      actions: [{ type: 'turn_on_for', entity_id: '', minutes: 5 }],
    },
  },
  {
    title: tr("Courrier arrivé"),
    detail: tr("Quand la boîte aux lettres s'ouvre : prévenir et lever l'état « Courrier ». Réarmé quand on relève le courrier (trappe arrière) ou à la main."),
    value: {
      name: tr("Courrier"),
      triggers: [{ type: 'state', entity_id: '', to: 'on' }],
      conditions: [],
      actions: [{ type: 'notify', message: tr("Le facteur est passé.") }],
      rest_state: false,
      rearm: { triggers: [{ type: 'state', entity_id: '', to: 'on' }], conditions: [] },
    },
  },
  {
    title: tr("Tout éteindre le soir"),
    detail: tr("À 23 h 30, éteindre les lumières choisies."),
    value: {
      name: tr("Extinction du soir"),
      triggers: [{ type: 'time', at: '23:30', days: [] }],
      conditions: [],
      actions: [{ type: 'command', entity_id: '', action: 'turn_off' }],
    },
  },
];

/** « Réarmé quand … », pour la liste. */
export function describeRearm(rearm, entities) {
  const parts = (rearm?.triggers ?? []).map((t) => describeTrigger(t, entities));
  if (rearm?.after_min) parts.push(tr("au bout de {v}", { v: rearm.after_min >= 60 && rearm.after_min % 60 === 0 ? `${rearm.after_min / 60} h` : `${rearm.after_min} min` }));
  return parts.join(' ou ');
}

/** « Quand Porte d'entrée devient ouverte » — pour la liste. */
export function describeTrigger(t, entities) {
  const name = (id) => entities[id]?.name ?? (id || '…');
  switch (t.type) {
    case 'state': {
      const base = `${name(t.entity_id)} devient ${stateLabel(entities[t.entity_id], t.to)}`;
      return t.for_s ? tr("{base} depuis {round} min", { base, round: Math.round(t.for_s / 60) }) : base;
    }
    case 'numeric': {
      const parts = [];
      if (t.above != null) parts.push(tr("dépasse {above}", { above: t.above }));
      if (t.below != null) parts.push(tr("passe sous {below}", { below: t.below }));
      return `${name(t.entity_id)} ${parts.join(' et ')}`;
    }
    case 'time': {
      const days = t.days?.length ? ` (${t.days.map((d) => DAYS[d - 1]).join(', ')})` : '';
      return `à ${t.at}${days}`;
    }
    case 'sun': {
      const what = t.event === 'sunrise' ? tr("au lever du soleil") : tr("au coucher du soleil");
      if (!t.offset_min) return what;
      return `${Math.abs(t.offset_min)} min ${t.offset_min < 0 ? 'avant' : tr("après")} ${what.slice(3)}`;
    }
  }
  return '';
}
