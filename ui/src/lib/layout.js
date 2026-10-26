// Cartes du tableau de bord, façon Mushroom : une grille en petites cases (4 de large sur
// téléphone), chaque carte de 1×1 (le quart d'une tuile d'avant) à 4×4. Fonctions pures.

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
  { w: 1, h: 1, label: 'Mini' },
  { w: 2, h: 1, label: 'Petit' },
  { w: 2, h: 2, label: 'Normal' },
  { w: 4, h: 1, label: 'Bandeau' },
  { w: 4, h: 2, label: 'Large' },
  { w: 4, h: 4, label: 'Très grand' },
];

export const LAYOUTS = [
  { id: 'auto', label: 'Automatique' },
  { id: 'vertical', label: 'Icône en haut' },
  { id: 'horizontal', label: 'Icône à gauche' },
  { id: 'icon', label: 'Icône seule' },
];

export const INFOS = [
  { id: 'state', label: 'L’état' },
  { id: 'last_changed', label: 'Depuis quand' },
  { id: 'both', label: 'Les deux' },
  { id: 'none', label: 'Rien' },
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
  };
}

/** Le texte secondaire selon le choix « infos affichées ». */
export function secondary(info, valueText, sinceText) {
  switch (info) {
    case 'none':
      return '';
    case 'last_changed':
      return sinceText ? `il y a ${sinceText}` : '';
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
  ]) {
    if (out[k] === def || out[k] === undefined) delete out[k];
  }
  for (const k of ['slider', 'graph']) if (out[k] === undefined || out[k] === null) delete out[k];
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
