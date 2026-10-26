// Cartes « Maison » : elles ne sont liées à aucun appareil précis et se recalculent toutes
// seules (une lampe ajoutée demain compte aussitôt dans « Toutes les lumières »).
// Fonctions pures, testées avec `npm test`.

import { describe as alarmView } from './security.js';

export const HOUSE_KINDS = [
  { kind: 'lights', name: 'Toutes les lumières', hint: 'Combien sont allumées ; un appui éteint tout', icon: 'bulb', w: 2, h: 1 },
  { kind: 'plugs', name: 'Toutes les prises', hint: 'Combien sont en marche ; un appui coupe tout', icon: 'plug', w: 2, h: 1 },
  { kind: 'room', name: 'Une pièce', hint: 'Les lumières d’une pièce, d’un appui', icon: 'sofa', w: 2, h: 1 },
  { kind: 'leave', name: 'Je pars', hint: 'Coupe lumières et prises, peut activer l’alarme', icon: 'door', w: 2, h: 1 },
  { kind: 'people', name: 'Qui est à la maison', hint: 'Les personnes présentes', icon: 'user', w: 2, h: 1 },
  { kind: 'alarm', name: 'Alarme', hint: 'Son état en couleur ; un appui l’ouvre', icon: 'shield', w: 2, h: 1 },
  { kind: 'fired', name: 'Déclenchés', hint: 'Les automatismes levés (courrier arrivé…), à réarmer', icon: 'alert', w: 2, h: 1 },
  { kind: 'batteries', name: 'Piles faibles', hint: 'Combien sont à changer', icon: 'battery', w: 2, h: 1 },
  { kind: 'offline', name: 'Injoignables', hint: 'Les appareils qui ne répondent plus', icon: 'wifi', w: 2, h: 1 },
  { kind: 'sun', name: 'Soleil', hint: 'Prochain lever ou coucher, calculé sans Internet', icon: 'sun', w: 2, h: 1 },
  { kind: 'clock', name: 'Horloge', hint: 'L’heure et la date', icon: 'timer', w: 2, h: 2 },
  { kind: 'scenes', name: 'Scènes', hint: 'Toutes les scènes, d’un appui', icon: 'scene', w: 4, h: 1 },
  { kind: 'timers', name: 'Minuteurs en cours', hint: 'Ceux qui tournent, avec leur décompte', icon: 'timer', w: 4, h: 1 },
];

export const kindInfo = (kind) => HOUSE_KINDS.find((k) => k.kind === kind) ?? { kind, name: 'Carte', icon: 'house', w: 2, h: 1 };

const BATTERY_LOW = 20;

const visible = (e) => !e.hidden;
const isOnValue = (e) => e.state?.value === 'on' || e.state?.value === true;

function plural(n, one, many) {
  return n === 1 ? one : many.replace('{n}', n);
}

/** Les entités que la carte commande (lumières, prises…). */
export function houseMembers(card, entities) {
  const all = entities.filter(visible);
  switch (card.kind) {
    case 'lights':
      return all.filter((e) => e.domain === 'light');
    case 'plugs':
      return all.filter((e) => e.domain === 'switch');
    case 'room':
      return all.filter((e) => e.domain === 'light' && (e.room || '') === (card.room || ''));
    case 'leave':
      return all.filter((e) => e.domain === 'light' || e.domain === 'switch');
    default:
      return [];
  }
}

function hhmm(iso) {
  return new Date(iso).toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' });
}

/** Le prochain lever ou coucher d'après la réponse de /api/sun. */
export function nextSun(sun, now = Date.now()) {
  const events = (sun?.days ?? [])
    .filter(Boolean)
    .flatMap((d) => [
      { kind: 'rise', at: Date.parse(d.rise) },
      { kind: 'set', at: Date.parse(d.set) },
    ])
    .filter((e) => e.at > now)
    .sort((a, b) => a.at - b.at);
  return events[0] ?? null;
}

/**
 * Une fausse entité pour la carte : la carte d'accueil l'affiche comme les autres
 * (valeur, texte, actif), et ses règles de couleur marchent pareil.
 * ctx = { entities: [...], security, sun, now }
 */
export function houseEntity(card, ctx) {
  const now = ctx.now ?? Date.now();
  const entities = ctx.entities ?? [];
  const info = kindInfo(card.kind);
  let value = 0;
  let text = '';
  let active = false;
  let members = [];
  let icon = info.icon;
  let tone = null;
  switch (card.kind) {
    case 'lights':
    case 'plugs':
    case 'room':
    case 'leave': {
      members = houseMembers(card, entities);
      const on = members.filter((e) => e.state?.available !== false && isOnValue(e));
      value = on.length;
      active = value > 0;
      if (card.kind === 'plugs') text = value ? plural(value, '1 en marche', '{n} en marche') : 'Toutes coupées';
      else if (card.kind === 'leave') text = value ? plural(value, '1 appareil en marche', '{n} appareils en marche') : 'Tout est éteint';
      else text = value ? plural(value, '1 allumée', '{n} allumées') : card.kind === 'room' ? 'Éteinte' : 'Toutes éteintes';
      if (!members.length) text = card.kind === 'room' && !card.room ? 'Choisissez la pièce' : 'Aucun appareil';
      break;
    }
    case 'people': {
      const people = entities.filter((e) => e.domain === 'person' && visible(e));
      const here = people.filter((p) => p.state?.value === 'home');
      value = here.length;
      active = value > 0;
      text = !people.length ? 'Aucune personne suivie' : here.length ? here.map((p) => p.name).join(', ') : 'Personne';
      members = people;
      break;
    }
    case 'alarm': {
      const v = alarmView(ctx.security?.status, now);
      value = ctx.security?.status?.phase === 'armed' ? ctx.security.status.mode : (ctx.security?.status?.phase ?? 'off');
      text = v.title + (v.line ? ` · ${v.line}` : '');
      active = v.tone !== 'off';
      tone = v.tone;
      icon = v.tone === 'off' ? 'shield' : 'shieldcheck';
      break;
    }
    case 'fired': {
      members = entities
        .filter((e) => e.domain === 'automation' && e.state?.attributes?.fired && e.state?.attributes?.enabled !== false)
        .sort((a, b) => a.name.localeCompare(b.name, 'fr'));
      value = members.length;
      active = value > 0;
      text = value ? members.map((e) => e.name).join(', ') : 'Rien à signaler';
      break;
    }
    case 'batteries': {
      members = entities.filter(
        (e) => visible(e) && e.capabilities?.kind === 'battery' && typeof e.state?.value === 'number' && e.state.value <= BATTERY_LOW,
      );
      value = members.length;
      text = value ? plural(value, '1 à changer', '{n} à changer') : 'Toutes bonnes';
      break;
    }
    case 'offline': {
      // Une ligne par appareil (pas par mesure) : on regroupe par device_id.
      const seen = new Map();
      for (const e of entities) {
        if (!visible(e) || e.state?.available !== false || e.integration === 'virtual') continue;
        const key = e.device_id || e.entity_id;
        if (!seen.has(key)) seen.set(key, e);
      }
      members = [...seen.values()];
      value = members.length;
      text = value ? plural(value, '1 ne répond plus', '{n} ne répondent plus') : 'Tout répond';
      break;
    }
    case 'sun': {
      const next = nextSun(ctx.sun, now);
      value = next?.kind ?? null;
      icon = next?.kind === 'set' ? 'sun' : 'moon';
      text = next ? `${next.kind === 'rise' ? 'Lever' : 'Coucher'} à ${hhmm(next.at)}` : 'Réglez le lieu';
      break;
    }
    case 'clock': {
      const d = new Date(now);
      value = d.toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' });
      text = d.toLocaleDateString('fr-FR', { weekday: 'long', day: 'numeric', month: 'long' });
      icon = 'timer';
      break;
    }
    default:
      text = '';
  }
  const SHORT = { lights: 'Lumières', plugs: 'Prises', batteries: 'Piles', offline: 'Injoignables', people: 'À la maison' };
  const name =
    SHORT[card.kind] ??
    (card.kind === 'room' ? card.room || 'Une pièce' : card.kind === 'sun' ? 'Soleil' : card.kind === 'clock' ? 'Horloge' : info.name);
  return {
    entity_id: `house.${card.kind}${card.room ? '_' + card.room : ''}`,
    domain: 'house',
    name,
    hidden: false,
    capabilities: { kind: `house_${card.kind}`, icon },
    state: { value, available: true, attributes: { text, active, tone }, last_changed: null },
    members,
  };
}

/** Ce que peut faire l'appui sur une carte Maison. */
export function houseActions(kind) {
  if (['lights', 'plugs', 'room'].includes(kind)) return ['off', 'on', 'toggle', 'details', 'none'];
  if (kind === 'leave') return ['run', 'details', 'none'];
  if (kind === 'fired') return ['details', 'rearm', 'none'];
  if (['people', 'batteries', 'offline', 'alarm'].includes(kind)) return ['details', 'none'];
  return ['none'];
}

export function houseDefaults(kind) {
  if (kind === 'lights' || kind === 'plugs') return { tap: 'off', hold: 'details' };
  if (kind === 'room') return { tap: 'toggle', hold: 'details' };
  if (kind === 'leave') return { tap: 'run', hold: 'details' };
  if (kind === 'fired') return { tap: 'details', hold: 'rearm' };
  if (['people', 'batteries', 'offline', 'alarm'].includes(kind)) return { tap: 'details', hold: 'details' };
  return { tap: 'none', hold: 'none' };
}

export function houseActionLabel(action, kind) {
  const lights = kind === 'lights' || kind === 'room';
  switch (action) {
    case 'off':
      return lights ? 'Tout éteindre' : 'Tout couper';
    case 'on':
      return lights ? 'Tout allumer' : 'Tout mettre en marche';
    case 'toggle':
      return 'Basculer (tout éteindre, sinon tout allumer)';
    case 'run':
      return 'Tout couper';
    case 'details':
      return 'Ouvrir la liste';
    case 'rearm':
      return 'Tout réarmer';
    default:
      return 'Rien';
  }
}

/**
 * Les commandes à envoyer : [{ entity_id, action }], ou une autre suite à donner.
 * { kind: 'commands', commands, arm? } | { kind: 'details' } | { kind: 'none' }
 */
export function resolveHouseAction(action, card, ent) {
  if (!action || action === 'none') return { kind: 'none' };
  if (action === 'details') return { kind: 'details' };
  if (action === 'rearm') {
    return { kind: 'rearm', automations: (ent?.members ?? []).map((e) => e.entity_id.slice('automation.'.length)) };
  }
  const members = (ent?.members ?? []).filter((e) => e.state?.available !== false);
  const anyOn = members.some(isOnValue);
  let turn = null;
  if (action === 'off' || action === 'run') turn = 'turn_off';
  else if (action === 'on') turn = 'turn_on';
  else if (action === 'toggle') turn = anyOn ? 'turn_off' : 'turn_on';
  if (!turn) return { kind: 'none' };
  const targets = members.filter((e) => (turn === 'turn_off' ? isOnValue(e) : !isOnValue(e)));
  return {
    kind: 'commands',
    commands: targets.map((e) => ({ entity_id: e.entity_id, action: turn })),
    arm: card.kind === 'leave' && card.arm ? 'away' : null,
  };
}

/** Couleurs d'office : piles et injoignables en rouge, alarme selon son état. */
export function houseRules(card) {
  if (card.kind === 'batteries' || card.kind === 'offline' || card.kind === 'fired') return [{ op: 'gt', value: 0, color: 'red' }];
  return [];
}
