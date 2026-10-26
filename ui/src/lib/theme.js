// Apparence : thème clair / sombre / celui de l'appareil, couleur de l'interface, taille du
// texte, arrondi, espacement, fond, et façon de changer de tableau (glisser ou onglets).
// L'apparence de la maison sert de base ; celle du compte, puis celle de l'écran s'y ajoutent.

export const DEFAULT_LOOK = {
  theme: 'system',
  accent: '',
  text: 'normal',
  radius: 'normal',
  density: 'normal',
  background: 'plain',
  nav: 'swipe',
};

/** Couleurs proposées pour l'interface ('' = la braise d'origine). */
export const ACCENTS = [
  { id: '', label: 'Braise', hex: '#f0a93a' },
  { id: '#e5484d', label: 'Rouge' },
  { id: '#f76b15', label: 'Orange' },
  { id: '#ffc53d', label: 'Soleil' },
  { id: '#46a758', label: 'Vert' },
  { id: '#12a594', label: 'Turquoise' },
  { id: '#0090ff', label: 'Ciel' },
  { id: '#3e63dd', label: 'Bleu' },
  { id: '#8e4ec6', label: 'Violet' },
  { id: '#d6409f', label: 'Rose' },
  { id: '#8d6e63', label: 'Bois' },
  { id: '#6f7b80', label: 'Ardoise' },
];

const CHOICES = {
  theme: ['system', 'light', 'dark'],
  text: ['normal', 'large'],
  radius: ['square', 'normal', 'round'],
  density: ['compact', 'normal', 'airy'],
  background: ['plain', 'gradient'],
  nav: ['swipe', 'tabs'],
};

function isHex(v) {
  return typeof v === 'string' && /^#[0-9a-f]{6}$/i.test(v);
}

/** Ne garde que ce qui est connu et valable. */
export function cleanLook(look) {
  const out = {};
  for (const [k, allowed] of Object.entries(CHOICES)) {
    if (allowed.includes(look?.[k])) out[k] = look[k];
  }
  if (look?.accent === '' || isHex(look?.accent)) out.accent = look.accent;
  return out;
}

/** L'apparence qui s'applique : défauts, puis maison, puis compte, puis écran. */
export function effectiveLook(...layers) {
  return Object.assign({}, DEFAULT_LOOK, ...layers.map(cleanLook));
}

function rgb(hex) {
  const n = parseInt(hex.slice(1), 16);
  return [n >> 16, (n >> 8) & 255, n & 255];
}

function toHex([r, g, b]) {
  return '#' + [r, g, b].map((x) => Math.round(Math.min(255, Math.max(0, x))).toString(16).padStart(2, '0')).join('');
}

/** Mélange deux couleurs (t = part de b). */
export function mix(a, b, t) {
  const A = rgb(a);
  const B = rgb(b);
  return toHex(A.map((x, i) => x + (B[i] - x) * t));
}

function luminance(hex) {
  const lin = (c) => {
    const x = c / 255;
    return x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
  };
  const [r, g, b] = rgb(hex);
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/** Les variables CSS de la couleur choisie : la teinte, sa lueur, l'encre lisible dessus. */
export function accentVars(accent) {
  if (!isHex(accent)) return null;
  const light = luminance(accent) > 0.36;
  return {
    '--ember': accent,
    '--ember-glow': mix(accent, '#ffffff', 0.45),
    '--ember-ink': light ? mix(accent, '#000000', 0.78) : '#ffffff',
  };
}

/** Applique l'apparence à la page (attributs sur <html>, variables de couleur). */
export function applyLook(look, root = document.documentElement) {
  root.dataset.theme = look.theme === 'system' ? '' : look.theme;
  if (!root.dataset.theme) delete root.dataset.theme;
  root.dataset.text = look.text;
  root.dataset.radius = look.radius;
  root.dataset.density = look.density;
  root.dataset.background = look.background;
  for (const k of ['--ember', '--ember-glow', '--ember-ink']) root.style.removeProperty(k);
  const vars = accentVars(look.accent);
  if (vars) for (const [k, v] of Object.entries(vars)) root.style.setProperty(k, v);
  // La barre d'état du téléphone prend la couleur du fond.
  const meta = document.querySelector('meta[name="theme-color"]');
  if (meta) meta.setAttribute('content', getComputedStyle(document.body).backgroundColor || '#edf0ee');
}
