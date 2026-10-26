// Liste les textes à traduire (tr("…")) qui manquent dans une langue.
// Usage : node scripts/i18n-keys.mjs en   → affiche les manquants, par fichier.
import fs from 'node:fs';
import path from 'node:path';

const lang = process.argv[2] || 'en';
const dict = (await import(path.resolve(`src/lib/lang/${lang}.js`))).default;
const re = /\btr\(\s*("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')/g;
const byFile = new Map();
const seen = new Set();
function walk(d) {
  for (const f of fs.readdirSync(d, { withFileTypes: true })) {
    const p = path.join(d, f.name);
    if (f.isDirectory()) {
      if (f.name !== 'lang') walk(p);
    } else if (/\.(svelte|js)$/.test(f.name) && !f.name.endsWith('.test.js') && f.name !== 'i18n.js') {
      const src = fs.readFileSync(p, 'utf8');
      let m;
      while ((m = re.exec(src))) {
        const lit = m[1].startsWith("'") ? '"' + m[1].slice(1, -1).replace(/\\'/g, "'").replace(/"/g, '\\"') + '"' : m[1];
        const k = JSON.parse(lit);
        if (seen.has(k) || k in dict) continue;
        seen.add(k);
        const rel = path.relative('src', p);
        if (!byFile.has(rel)) byFile.set(rel, []);
        byFile.get(rel).push(k);
      }
    }
  }
}
walk('src');
for (const [f, keys] of byFile) {
  console.log(`// ${f}`);
  for (const k of keys) console.log(JSON.stringify(k));
}
console.error(`${seen.size} manquant(s)`);
