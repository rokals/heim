// Cartes du tableau de bord, façon Mushroom : une grille en petites cases (4 de large sur
// téléphone), chaque carte de 1×1 (le quart d'une tuile d'avant) à 4×4. Fonctions pures.

import { tr } from './i18n.js';
/** Ancien format (size: 1 | 2) ou rien : la taille en cases (largeur w, hauteur h). */
export function cardSize(card, entity) {
  const clamp = (n) => Math.min(4, Math.max(1, Math.round(Number(n) || 0)));
  if (card?.type === 'title') return { w: clamp(card.w ?? 4), h: clamp(card.h ?? 1) };
  if (card?.type === 'spacer') return { w: clamp(card.w ?? 1), h: clamp(card.h ?? 1) };
  if (card?.w != null || card?.h != null) return { w: clamp(card.w ?? 2), h: clamp(card.h ?? 2) };
  const d = String(card?.entity_id ?? entity?.entity_id ?? '').split('.')[0];
  const reading = d === 'sensor' || d === 'binary_sensor';
  if (card?.size === 2) return reading ? { w: 4, h: 1 } : { w: 4, h: 2 };
  return reading ? { w: 4, h: 1 } : { w: 2, h: 2 };
}

export const SIZE_PRESETS = [
  { w: 1, h: 1, label: tr("Mini") },
  { w: 2, h: 1, label: tr("Petit") },
  { w: 2, h: 2, label: tr("Normal") },
  { w: 4, h: 1, label: tr("Bandeau") },
  { w: 4, h: 2, label: tr("Large") },
  { w: 4, h: 4, label: tr("Très grand") },
];

export const LAYOUTS = [
  { id: 'auto', label: tr("Automatique") },
  { id: 'vertical', label: tr("Icône en haut") },
  { id: 'horizontal', label: tr("Icône à gauche") },
  { id: 'icon', label: tr("Icône seule") },
];

export const INFOS = [
  { id: 'state', label: tr("L’état") },
  { id: 'last_changed', label: tr("Depuis quand") },
  { id: 'both', label: tr("Les deux") },
  { id: 'none', label: tr("Rien") },
];

/** La disposition réelle : « auto » choisit selon la taille. */
export function resolveLayout(card, w, h) {
  const l = card?.layout ?? 'auto';
  if (l !== 'auto') return l;
  if (w === 1 && h === 1) return 'icon';
  if (h === 1) return 'horizontal';
  if (w === 1) return 'icon';
  return 'vertical';
}

function domain(entity) {
  return String(entity?.entity_id ?? '').split('.')[0];
}

/** Curseur de luminosité dans la carte : lampes réglables, carte assez grande. */
export function showSlider(card, entity, w, h) {
  const d = domain(entity);
  const dimmable = d === 'light' && Boolean(entity?.capabilities?.brightness);
  if (!dimmable || card?.slider === false) return false;
  if (card?.slider === true) return w >= 2;
  return (w >= 2 && h >= 2) || w >= 4;
}

/** Petite courbe des dernières 24 h : capteurs numériques, carte assez grande. */
export function showGraph(card, entity, w, h) {
  if (domain(entity) !== 'sensor' || typeof entity?.state?.value !== 'number' || card?.graph === false) return false;
  if (card?.graph === true) return w >= 2;
  return w >= 3 || h >= 3 || (w >= 2 && h >= 2);
}

/** Les formes d'une valeur numérique : le chiffre seul, une barre, un anneau. */
export const GAUGES = ['value', 'bar', 'ring'];

/**
 * L'échelle d'une jauge : réglée sur la tuile, sinon donnée par l'appareil, sinon devinée
 * d'après l'unité (un pourcentage va de 0 à 100).
 */
export function gaugeRange(card, entity) {
  const caps = entity?.capabilities ?? {};
  const kind = caps.kind;
  const unit = caps.unit;
  let [min, max] = [0, 100];
  if (kind === 'temperature' || unit === '°C') [min, max] = caps.kind === 'temperature' && entity?.integration === 'system' ? [20, 90] : [0, 40];
  else if (kind === 'power' || unit === 'W') [min, max] = [0, 3000];
  else if (kind === 'co2' || unit === 'ppm') [min, max] = [400, 2000];
  else if (kind === 'illuminance' || unit === 'lx') [min, max] = [0, 1000];
  if (typeof caps.min === 'number') min = caps.min;
  if (typeof caps.max === 'number') max = caps.max;
  if (card?.min !== undefined && card.min !== '' && !Number.isNaN(Number(card.min))) min = Number(card.min);
  if (card?.max !== undefined && card.max !== '' && !Number.isNaN(Number(card.max))) max = Number(card.max);
  if (max <= min) max = min + 1;
  return { min, max };
}

/** La part remplie (0 à 1) d'une jauge. */
export function gaugeFraction(value, { min, max }) {
  if (typeof value !== 'number' || Number.isNaN(value)) return 0;
  return Math.min(1, Math.max(0, (value - min) / (max - min)));
}

/** Tout ce qu'une carte affiche, défauts compris. */
export function cardConfig(card, entity) {
  const { w, h } = cardSize(card, entity);
  const layout = resolveLayout(card, w, h);
  return {
    w,
    h,
    layout,
    showName: card?.show_name !== false && layout !== 'icon',
    showState: card?.show_state !== false && layout !== 'icon',
    showIcon: card?.show_icon !== false,
    info: card?.info ?? 'state',
    fill: card?.fill ?? 'tint',
    slider: layout !== 'icon' && showSlider(card, entity, w, h),
    graph: layout !== 'icon' && showGraph(card, entity, w, h),
    gauge: domain(entity) === 'sensor' && typeof entity?.state?.value === 'number' && GAUGES.includes(card?.gauge) && card.gauge !== 'value' ? card.gauge : null,
  };
}

/** Le texte secondaire selon le choix « infos affichées ». */
export function secondary(info, valueText, sinceText) {
  switch (info) {
    case 'none':
      return '';
    case 'last_changed':
      return sinceText ? tr("il y a {sinceText}", { sinceText }) : '';
    case 'both':
      return sinceText ? `${valueText} · ${sinceText}` : valueText;
    default:
      return valueText;
  }
}

/**
 * Redimensionner à la poignée : la taille en cases d'après la taille en pixels.
 * `unit` = largeur d'une colonne, `row` = hauteur d'une rangée, `gap` = écart.
 */
export function sizeFromPixels(px, py, { unit, row, gap, cols }) {
  const w = Math.round((px + gap) / (unit + gap));
  const h = Math.round((py + gap) / (row + gap));
  return { w: Math.min(Math.min(4, cols), Math.max(1, w)), h: Math.min(4, Math.max(1, h)) };
}

/** Ce qu'on enregistre : on retire les valeurs par défaut pour garder le fichier lisible. */
export function cleanAppearance(card) {
  const out = { ...card };
  delete out.size;
  for (const [k, def] of [
    ['layout', 'auto'],
    ['show_name', true],
    ['show_state', true],
    ['show_icon', true],
    ['info', 'state'],
    ['fill', 'tint'],
    ['gauge', 'value'],
  ]) {
    if (out[k] === def || out[k] === undefined) delete out[k];
  }
  for (const k of ['slider', 'graph']) if (out[k] === undefined || out[k] === null) delete out[k];
  for (const k of ['min', 'max']) {
    if (!out.gauge || out[k] === undefined || out[k] === null || String(out[k]).trim() === '' || Number.isNaN(Number(out[k]))) delete out[k];
    else out[k] = Number(out[k]);
  }
  return out;
}

/** Le tracé d'une petite courbe (ligne et surface) dans un cadre W×H, sans axes. */
export function sparkPath(points, W = 100, H = 30) {
  if (!points || points.length < 2) return null;
  const vs = points.map((p) => p.v);
  let lo = Math.min(...vs);
  let hi = Math.max(...vs);
  if (hi - lo < 1e-9) {
    hi += 0.5;
    lo -= 0.5;
  }
  const t0 = points[0].t;
  const t1 = Math.max(points.at(-1).t, t0 + 1);
  const x = (t) => (((t - t0) / (t1 - t0)) * W).toFixed(1);
  const y = (v) => (2 + (1 - (v - lo) / (hi - lo)) * (H - 4)).toFixed(1);
  const line = points.map((p, i) => `${i ? 'L' : 'M'}${x(p.t)},${y(p.v)}`).join('');
  return { line, area: `${line}L${W},${H}L0,${H}Z` };
}

/** L'encre lisible sur un fond plein de cette couleur : sombre sur clair, blanche sur foncé. */
export function inkFor(hex) {
  const m = /^#?([0-9a-f]{6})$/i.exec(String(hex ?? ''));
  if (!m) return '#fff';
  const n = parseInt(m[1], 16);
  const lin = (c) => {
    const x = c / 255;
    return x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
  };
  const L = 0.2126 * lin(n >> 16) + 0.7152 * lin((n >> 8) & 255) + 0.0722 * lin(n & 255);
  return L > 0.4 ? '#2a1904' : '#fff';
}
