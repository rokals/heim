import test from 'node:test';
import assert from 'node:assert/strict';
import { controlLabel, groupControls, hsToHex, kelvin, valueAt, valueLabel, xyToHex } from './controls.js';

test('noms et valeurs en français', () => {
  assert.equal(controlLabel({ key: 'power_on_behavior_l1', name: 'power_on_behavior' }), 'Après une coupure de courant');
  assert.equal(controlLabel({ key: 'level_config.on_level', name: 'on_level' }), 'Luminosité à l’allumage');
  assert.equal(controlLabel({ key: 'x', name: 'temperature_offset_fancy', label: 'temperature_offset_fancy' }), 'Temperature offset fancy');
  assert.equal(valueLabel('previous'), 'Comme avant');
  assert.equal(valueLabel(true), 'Oui');
});

test('rangement : lumière, commandes, réglages', () => {
  const g = groupControls([
    { key: 'state', name: 'state', kind: 'binary', group: 'light' },
    { key: 'brightness', name: 'brightness', kind: 'numeric', group: 'light' },
    { key: 'color', name: 'color_xy', kind: 'color', group: 'light' },
    { key: 'effect', name: 'effect', kind: 'enum' },
    { key: 'power_on_behavior', name: 'power_on_behavior', kind: 'enum', category: 'config' },
    { key: 'state_l2', name: 'state', kind: 'binary', group: 'switch', endpoint: 'l2' },
  ]);
  assert.deepEqual(Object.keys(g.light).sort(), ['brightness', 'color', 'effect', 'state']);
  assert.equal(g.main[0].key, 'state_l2');
  assert.equal(g.settings[0].key, 'power_on_behavior');
});

test('conversions', () => {
  assert.equal(valueAt({ level_config: { on_level: 42 } }, 'level_config.on_level'), 42);
  assert.equal(kelvin(153), 6550);
  assert.equal(hsToHex({ hue: 0, saturation: 100 }), '#ff0000');
  assert.match(xyToHex({ x: 0.7, y: 0.3 }), /^#ff/);
});
