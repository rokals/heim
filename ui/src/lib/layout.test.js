import { test } from 'node:test';
import assert from 'node:assert/strict';
import { cardConfig, cardSize, cleanAppearance, resolveLayout, secondary, sizeFromPixels } from './layout.js';

test('les anciennes cartes gardent leur taille', () => {
  assert.deepEqual(cardSize({ entity_id: 'light.a', size: 1 }), { w: 2, h: 2 });
  assert.deepEqual(cardSize({ entity_id: 'light.a', size: 2 }), { w: 4, h: 2 });
  assert.deepEqual(cardSize({ entity_id: 'sensor.t', size: 2 }), { w: 4, h: 1 });
  assert.deepEqual(cardSize({ entity_id: 'light.a', w: 9, h: 0 }), { w: 4, h: 1 }, 'bornée');
});

test('disposition automatique selon la taille', () => {
  assert.equal(resolveLayout({}, 1, 1), 'icon');
  assert.equal(resolveLayout({}, 4, 1), 'horizontal');
  assert.equal(resolveLayout({}, 2, 2), 'vertical');
  assert.equal(resolveLayout({ layout: 'horizontal' }, 2, 2), 'horizontal');
});

test('curseur et courbe seulement quand ça a de la place', () => {
  const lamp = { entity_id: 'light.a', capabilities: { brightness: true } };
  assert.equal(cardConfig({ entity_id: 'light.a', w: 2, h: 2 }, lamp).slider, true);
  assert.equal(cardConfig({ entity_id: 'light.a', w: 2, h: 1 }, lamp).slider, false);
  assert.equal(cardConfig({ entity_id: 'light.a', w: 2, h: 2, slider: false }, lamp).slider, false);
  const t = { entity_id: 'sensor.t', state: { value: 21 } };
  assert.equal(cardConfig({ entity_id: 'sensor.t', w: 4, h: 1 }, t).graph, true);
  assert.equal(cardConfig({ entity_id: 'sensor.t', w: 2, h: 1 }, t).graph, false);
});

test('texte secondaire', () => {
  assert.equal(secondary('both', '21 °C', '5 min'), '21 °C · 5 min');
  assert.equal(secondary('last_changed', '21 °C', '5 min'), 'il y a 5 min');
  assert.equal(secondary('none', '21 °C', '5 min'), '');
});

test('poignée : pixels vers cases', () => {
  const g = { unit: 80, row: 70, gap: 10, cols: 4 };
  assert.deepEqual(sizeFromPixels(170, 150, g), { w: 2, h: 2 });
  assert.deepEqual(sizeFromPixels(30, 20, g), { w: 1, h: 1 });
  assert.deepEqual(sizeFromPixels(900, 900, g), { w: 4, h: 4 });
});

test('les défauts ne sont pas enregistrés', () => {
  assert.deepEqual(cleanAppearance({ entity_id: 'x', size: 1, w: 2, h: 2, layout: 'auto', info: 'state', fill: 'solid' }), {
    entity_id: 'x',
    w: 2,
    h: 2,
    fill: 'solid',
  });
});

test('petite courbe', async () => {
  const { sparkPath } = await import('./layout.js');
  assert.equal(sparkPath([{ t: 0, v: 1 }]), null);
  const p = sparkPath([{ t: 0, v: 0 }, { t: 10, v: 10 }], 100, 30);
  assert.equal(p.line, 'M0.0,28.0L100.0,2.0');
});

test('encre lisible', async () => {
  const { inkFor } = await import('./layout.js');
  assert.equal(inkFor('#ffd23f'), '#2a1904');
  assert.equal(inkFor('#3e63dd'), '#fff');
});
