// Langues de l'interface. Le texte français sert de clé (comme gettext) : tr('Allumer')
// renvoie « Allumer » en français et « Turn on » en anglais. Un texte absent d'un fichier de
// langue reste en français : rien ne casse, il manque juste une traduction.
//
// Ajouter une langue : copier lang/en.js, traduire chaque valeur, l'ajouter à LANGS et DICTS.
// `npm test` signale les textes qui n'ont pas encore de traduction.
//
// La langue est fixée au chargement de la page (celle du compte, de l'écran, sinon du
// navigateur) ; si elle change, la page se recharge : plus simple et plus sûr qu'un
// changement à chaud de chaque texte.

import en from './lang/en.js';

export const LANGS = [
  { id: 'fr', label: 'Français', locale: 'fr-FR' },
  { id: 'en', label: 'English', locale: 'en-GB' },
];

const DICTS = { fr: null, en };
const KEY = 'heim_lang';

function stored() {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

/** La langue du navigateur, si Heim la connaît (sinon l'anglais). */
export function browserLang() {
  const list = typeof navigator === 'undefined' ? [] : (navigator.languages ?? [navigator.language]);
  for (const l of list) {
    const id = String(l || '').slice(0, 2).toLowerCase();
    if (id in DICTS) return id;
  }
  return 'en';
}

function known(id) {
  return typeof id === 'string' && id in DICTS ? id : null;
}

// Hors navigateur (tests) : le français, langue des textes sources.
let current = known(stored()) ?? (typeof document === 'undefined' ? 'fr' : browserLang());

/** Une langue déjà retenue sur cet appareil ? */
export function hasStoredLang() {
  return known(stored()) !== null;
}

export function currentLang() {
  return current;
}

/** Pour les dates et les nombres (« 21,5 » / « 21.5 », « lundi » / « Monday »). */
export function locale() {
  return LANGS.find((l) => l.id === current)?.locale ?? 'fr-FR';
}

/**
 * Change de langue. Renvoie true si la page va se recharger pour l'appliquer.
 * reload: false pour seulement la retenir (tests, premier écran).
 */
export function setLang(id, { reload = true } = {}) {
  const next = known(id);
  if (!next) return false;
  try {
    localStorage.setItem(KEY, next);
  } catch {
    /* navigation privée */
  }
  if (next === current) return false;
  current = next;
  if (reload && typeof location !== 'undefined') {
    location.reload();
    return true;
  }
  return false;
}

/** Pour les tests : traduire avec une langue donnée sans toucher au reste. */
export function translate(lang, text, vars) {
  const dict = DICTS[lang];
  let s = (dict && dict[text]) || text;
  if (vars) s = s.replace(/\{(\w+)\}/g, (m, k) => (vars[k] ?? m));
  return s;
}

/** Traduit un texte français ; {nom} est remplacé par vars.nom. (« tr » : court, et sans
 *  risque de confusion avec une variable « t » du code.) */
export function tr(text, vars) {
  return translate(current, text, vars);
}

export const t = tr;

/** Les textes d'une langue (pour le test de couverture). */
export function dictionary(lang) {
  return DICTS[lang];
}
