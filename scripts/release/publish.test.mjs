import test from 'node:test';
import assert from 'node:assert/strict';
import { assertRemoteReady, expectedAssets } from './core.mjs';
const tag = 'v0.1.0', body = 'Frozen English.\n\n---\n\n冻结中文。\n';
const remote = () => ({ tagName: tag, body, assets: expectedAssets('0.1.0').map(name => ({ name })) });
test('remote gate accepts exactly the frozen tag, nine assets and complete notes', () => {
  assert.doesNotThrow(() => assertRemoteReady(remote(), tag, body));
});
for (const [name, change, pattern] of [
  ['additional updater', r => r.assets.push({ name: 'latest.json' }), /asset set/],
  ['additional signature', r => r.assets.push({ name: 'old-updater.sig' }), /asset set/],
  ['duplicate', r => r.assets.push(r.assets[0]), /asset set/],
  ['missing architecture', r => r.assets.pop(), /asset set/],
  ['wrong tag', r => r.tagName = 'v0.2.0', /remote tag/],
  ['incomplete notes', r => r.body = 'Frozen English.\n', /release body/],
]) test(`remote gate rejects ${name}`, () => {
  const r = remote(); change(r); assert.throws(() => assertRemoteReady(r, tag, body), pattern);
});
