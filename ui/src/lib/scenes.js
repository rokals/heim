// Scènes : mots, modèles et petites conversions de l'éditeur. Fonctions pures (npm test).

import { tr, locale } from './i18n.js';
export const toKelvin = (mired) => Math.round(1e6 / mired / 50) * 50;
export const toMired = (k) => Math.round(1e6 / k);

/** Plage de blanc d'une lampe, en mireds : [chaud…froid] inversé, [min, max]. */
export function tempRange(entity) {
  const r = entity?.capabilities?.color_temp;
  return Array.isArray(r) && r.length === 2 ? r : null;
}

export function dimmable(entity) {
  return entity?.domain === 'light' && Boolean(entity?.capabilities?.brightness);
}

/** « Allumé · 40 % · 2 700 K », « Éteint », « Allumé · couleur ». */
export function summary(target, entity) {
  if (!target) return '';
  if (!target.on) return tr("Éteint");
  const parts = [tr("Allumé")];
  if (target.brightness != null) parts.push(`${Math.max(1, Math.round((target.brightness / 255) * 100))} %`);
  if (target.color) parts.push('couleur');
  else if (target.color_temp != null) parts.push(`${String(toKelvin(target.color_temp)).replace(/\B(?=(\d{3})+$)/g, '\u00a0')}\u00a0K`);
  if (target.effect) parts.push(`effet ${target.effect}`);
  if (parts.length === 1 && entity?.domain === 'light') parts.push(tr("sans changer le reste"));
  return parts.join(' · ');
}

/** Ce qu'on envoie au serveur : seulement les réglages cochés. */
export function cleanTarget(t) {
  if (!t.on) return { on: false };
  const out = { on: true };
  if (t.brightness != null) out.brightness = Math.round(Math.min(255, Math.max(1, t.brightness)));
  if (t.color) out.color = t.color;
  else if (t.color_temp != null) out.color_temp = Math.round(t.color_temp);
  if (t.effect) out.effect = t.effect;
  return out;
}

export function sameTarget(a, b) {
  return JSON.stringify(cleanTarget(a ?? { on: false })) === JSON.stringify(cleanTarget(b ?? { on: false }));
}

/** Modèles prêts : luminosité en %, blanc en kelvins (ramené à ce que sait faire chaque lampe). */
export const TEMPLATES = [
  { id: 'soiree', name: tr("Soirée"), icon: 'sofa', color: '#f2a541', pct: 45, kelvin: 2700, hint: tr("Chaud et doux") },
  { id: 'lecture', name: tr("Lecture"), icon: 'bulb', color: '#ffd23f', pct: 100, kelvin: 4000, hint: tr("Lumière franche") },
  { id: 'cine', name: tr("Ciné"), icon: 'moon', color: '#8e4ec6', pct: 12, kelvin: 2200, hint: tr("Presque éteint") },
  { id: 'nuit', name: tr("Nuit"), icon: 'moon', color: '#3e63dd', pct: 5, kelvin: 2200, hint: tr("Veilleuse") },
  { id: 'reveil', name: tr("Réveil"), icon: 'sun', color: '#ffb224', pct: 70, kelvin: 4000, hint: tr("Pour se lever") },
];

/** Les réglages d'un modèle pour une lampe donnée. */
export function templateTarget(tpl, entity) {
  if (entity?.domain === 'switch') return { on: tpl.pct > 20 };
  const t = { on: true };
  if (dimmable(entity) || entity?.domain === 'group') t.brightness = Math.round((tpl.pct / 100) * 255);
  const range = tempRange(entity);
  if (range) t.color_temp = Math.min(range[1], Math.max(range[0], toMired(tpl.kelvin)));
  return t;
}

/** Ce qui peut faire partie d'une scène. */
export function sceneCandidates(entities) {
  return entities
    .filter((e) => ['light', 'switch', 'group'].includes(e.domain) && !e.hidden && e.category !== 'diagnostic')
    .sort((a, b) => (a.room || '~').localeCompare(b.room || '~', 'fr') || a.name.localeCompare(b.name, 'fr'));
}

/** « Active jusqu'à 22:40 », « Active, en partie », « Inactive ». */
export function sceneStatus(entity, now = Date.now()) {
  const on = entity?.state?.value === 'on';
  if (!on) return tr("Inactive");
  const until = entity?.state?.attributes?.until;
  if (until && new Date(until).getTime() > now) {
    const t = new Date(until).toLocaleTimeString(locale(), { hour: '2-digit', minute: '2-digit' });
    return tr("Active jusqu'à {t}", { t });
  }
  return entity?.state?.attributes?.partial ? tr("Active, en partie") : tr("Active");
}
