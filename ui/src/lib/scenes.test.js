import { test } from 'node:test';
import assert from 'node:assert/strict';
import { cleanTarget, sameTarget, sceneStatus, summary, templateTarget, TEMPLATES, toKelvin, toMired } from './scenes.js';

test('kelvins et mireds', () => {
  assert.equal(toKelvin(370), 2700);
  assert.equal(toMired(4000), 250);
});

test('seuls les réglages cochés partent', () => {
  assert.deepEqual(cleanTarget({ on: false, brightness: 100 }), { on: false });
  assert.deepEqual(cleanTarget({ on: true, brightness: 100.4, color: '#ff0000', color_temp: 300 }), {
    on: true,
    brightness: 100,
    color: '#ff0000',
  });
  assert.ok(sameTarget({ on: true, brightness: 10 }, { on: true, brightness: 10, effect: '' }));
});

test('résumé lisible', () => {
  const lamp = { domain: 'light' };
  assert.equal(summary({ on: true, brightness: 102, color_temp: 370 }, lamp), 'Allumé · 40 % · 2\u00a0700\u00a0K');
  assert.equal(summary({ on: true }, lamp), 'Allumé · sans changer le reste');
  assert.equal(summary({ on: false }, lamp), 'Éteint');
});

test('modèle ramené à ce que sait faire la lampe', () => {
  const cine = TEMPLATES.find((t) => t.id === 'cine');
  const lamp = { domain: 'light', capabilities: { brightness: true, color_temp: [250, 454] } };
  const t = templateTarget(cine, lamp);
  assert.equal(t.color_temp, 454, 'pas plus chaud que la lampe');
  assert.equal(t.brightness, 31);
  assert.deepEqual(templateTarget(cine, { domain: 'switch' }), { on: false });
});

test('état de la scène', () => {
  const now = new Date('2026-10-26T20:00:00Z').getTime();
  assert.equal(sceneStatus({ state: { value: 'off' } }, now), 'Inactive');
  assert.match(sceneStatus({ state: { value: 'on', attributes: { until: '2026-10-26T22:00:00Z' } } }, now), /^Active jusqu'à/);
});
