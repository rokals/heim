import { test } from 'node:test';
import assert from 'node:assert/strict';
import { accentVars, cleanLook, effectiveLook, mix } from './theme.js';

test('apparence : couches et nettoyage', () => {
  const look = effectiveLook({ theme: 'dark', nav: 'tabs' }, { theme: 'light', accent: '#3e63dd' }, { text: 'large', bidule: 1 });
  assert.equal(look.theme, 'light');
  assert.equal(look.nav, 'tabs');
  assert.equal(look.text, 'large');
  assert.equal(look.accent, '#3e63dd');
  assert.deepEqual(cleanLook({ theme: 'violet', accent: 'red' }), {});
});

test('couleur : lueur et encre lisible', () => {
  assert.equal(mix('#000000', '#ffffff', 0.5), '#808080');
  assert.equal(accentVars('#3e63dd')['--ember-ink'], '#ffffff');
  assert.notEqual(accentVars('#ffc53d')['--ember-ink'], '#ffffff');
  assert.equal(accentVars(''), null);
});
