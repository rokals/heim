import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { dictionary, translate, LANGS } from './i18n.js';

/** Tous les textes passés à tr("…") dans l'interface. */
export function sourceKeys(dir = path.resolve('src')) {
  const keys = new Map();
  const re = /\btr\(\s*("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')/g;
  const walk = (d) => {
    for (const f of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, f.name);
      if (f.isDirectory()) {
        if (f.name !== 'lang') walk(p);
      } else if (/\.(svelte|js)$/.test(f.name) && !f.name.endsWith('.test.js') && f.name !== 'i18n.js') {
        const src = fs.readFileSync(p, 'utf8');
        let m;
        while ((m = re.exec(src))) {
          const lit = m[1].startsWith("'") ? '"' + m[1].slice(1, -1).replace(/\\'/g, "'").replace(/"/g, '\\"') + '"' : m[1];
          const key = JSON.parse(lit);
          if (!keys.has(key)) keys.set(key, path.relative(dir, p));
        }
      }
    }
  };
  walk(dir);
  return keys;
}

const vars = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

test('chaque langue traduit tous les textes, avec les mêmes {variables}', () => {
  const keys = sourceKeys();
  assert.ok(keys.size > 500, 'textes trouvés');
  for (const { id } of LANGS) {
    const dict = dictionary(id);
    if (!dict) continue;
    const missing = [...keys.keys()].filter((k) => !(k in dict));
    assert.deepEqual(missing.slice(0, 20), [], `${id} : ${missing.length} texte(s) sans traduction`);
    for (const [k, v] of Object.entries(dict)) {
      assert.deepEqual(vars(v), vars(k), `${id} : variables différentes pour « ${k} »`);
    }
  }
});

test('remplacement des variables et repli sur le français', () => {
  assert.equal(translate('en', 'Allumé'), dictionary('en')['Allumé']);
  assert.equal(translate('en', 'Texte inconnu {n}', { n: 3 }), 'Texte inconnu 3');
  assert.equal(translate('fr', '{n} allumées', { n: 2 }), '2 allumées');
});
