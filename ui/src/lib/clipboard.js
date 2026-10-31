// Copier du texte. L'API du presse-papiers n'existe qu'en HTTPS : sur http://heim.local elle
// manque, donc on passe par l'ancienne méthode (zone de texte sélectionnée + copie), qui marche
// aussi sur iPhone tant qu'on est dans le geste de l'utilisateur.
export async function copyText(text) {
  if (globalThis.isSecureContext && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      /* on essaie l'autre méthode */
    }
  }
  const area = document.createElement('textarea');
  area.value = text;
  area.setAttribute('readonly', '');
  area.style.position = 'fixed';
  area.style.top = '0';
  area.style.left = '0';
  area.style.opacity = '0';
  area.style.fontSize = '16px'; // iPhone : pas de zoom sur la zone
  document.body.appendChild(area);
  area.focus();
  area.select();
  area.setSelectionRange(0, text.length);
  let ok = false;
  try {
    ok = document.execCommand('copy');
  } finally {
    area.remove();
  }
  if (!ok) throw new Error('copie impossible');
}
