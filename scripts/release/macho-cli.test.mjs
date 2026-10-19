import test from 'node:test';
import assert from 'node:assert/strict';
import { assertCliSigningEquivalent } from './macho-cli.mjs';

// Structural fixtures only: production inspectApp owns real codesign validation.
function cli(target, large = false) {
  const arm = target === 'aarch64-apple-darwin', page = arm ? 16384 : 4096;
  const start = page + 64, size = large ? page + 64 : 64, bytes = Buffer.alloc(start + size);
  for (const [offset, value] of [[0, 0xfeedfacf], [4, arm ? 0x0100000c : 0x01000007], [12, 2], [16, 3], [20, 160]]) bytes.writeUInt32LE(value, offset);
  for (const [offset, name, fileoff, filesize] of [[32, '__TEXT', 0, page], [104, '__LINKEDIT', page, bytes.length - page]]) {
    bytes.writeUInt32LE(0x19, offset); bytes.writeUInt32LE(72, offset + 4); bytes.write(name, offset + 8);
    bytes.writeBigUInt64LE(0x100000000n + BigInt(fileoff), offset + 24);
    bytes.writeBigUInt64LE(BigInt(Math.ceil(filesize / page) * page), offset + 32);
    bytes.writeBigUInt64LE(BigInt(fileoff), offset + 40); bytes.writeBigUInt64LE(BigInt(filesize), offset + 48);
  }
  bytes.writeUInt32LE(0x1d, 176); bytes.writeUInt32LE(16, 180); bytes.writeUInt32LE(start, 184); bytes.writeUInt32LE(size, 188);
  bytes.fill(0x45, 192, start); bytes.fill(large ? 0xa1 : 0xb2, start);
  return bytes;
}
const arm = 'aarch64-apple-darwin';
for (const target of [arm, 'x86_64-apple-darwin']) test(`${target} permits only terminal signature and its three derived sizes`, () => {
  const staged = cli(target, true), bundled = cli(target), before = [Buffer.from(staged), Buffer.from(bundled)];
  assert.doesNotThrow(() => assertCliSigningEquivalent(staged, bundled, target));
  assert.doesNotThrow(() => assertCliSigningEquivalent(staged, staged, target));
  assert.deepEqual([staged, bundled], before);
});
for (const [name, mutate] of [
  ['wrong magic', b => b.writeUInt32LE(0xcafebabe, 0)],
  ['wrong CPU', b => b.writeUInt32LE(0x01000007, 4)],
  ['wrong filetype', b => b.writeUInt32LE(6, 12)],
  ['count exceeds command bytes', b => b.writeUInt32LE(1000, 16)],
  ['truncated load area', b => b.writeUInt32LE(b.length, 20)],
  ['unaligned command', b => b.writeUInt32LE(71, 36)],
  ['section count disagrees', b => b.writeUInt32LE(1, 96)],
  ['load end mismatch', b => b.writeUInt32LE(168, 20)],
  ['missing signature', b => b.writeUInt32LE(0x1e, 176)],
  ['duplicate signature', b => { b.copy(b, 192, 176, 192); b.writeUInt32LE(4, 16); b.writeUInt32LE(176, 20); }],
  ['missing LINKEDIT', b => b.write('OTHER', 112)],
  ['duplicate LINKEDIT', b => { b.fill(0, 40, 56); b.write('__LINKEDIT', 40); }],
  ['signature not at EOF', b => b.writeUInt32LE(63, 188)],
  ['empty signature', b => b.writeUInt32LE(0, 188)],
  ['signature overlaps load commands', b => b.writeUInt32LE(176, 184)],
  ['wrong LINKEDIT filesize', b => b.writeBigUInt64LE(127n, 152)],
  ['wrong LINKEDIT vmsize', b => b.writeBigUInt64LE(32768n, 136)],
  ['LINKEDIT after signature', b => b.writeBigUInt64LE(BigInt(b.length), 144)],
  ['other segment overlaps LINKEDIT', b => b.writeBigUInt64LE(16385n, 80)],
  ['segment VM overflow', b => b.writeBigUInt64LE(0xffffffffffffffffn, 56)],
]) test(`rejects ${name}, including when both physical inputs are equal`, () => {
  const bad = cli(arm); mutate(bad);
  assert.throws(() => assertCliSigningEquivalent(cli(arm, true), bad, arm), /CLI hash/);
  assert.throws(() => assertCliSigningEquivalent(bad, bad, arm), /CLI hash/);
});
for (const [name, mutate] of [
  ['header flags', b => b.writeUInt32LE(1, 24)],
  ['executable byte', b => { b[256] ^= 1; }],
  ['LINKEDIT non-signature byte', b => { b[16384] ^= 1; }],
  ['signature offset', b => { b.writeUInt32LE(16447, 184); b.writeUInt32LE(65, 188); }],
]) test(`rejects otherwise structured ${name} changes`, () => {
  const bundled = cli(arm); mutate(bundled);
  assert.throws(() => assertCliSigningEquivalent(cli(arm, true), bundled, arm), /non-signature bytes changed/);
});
test('rejects truncation and unsupported target', () => {
  assert.throws(() => assertCliSigningEquivalent(Buffer.alloc(31), Buffer.alloc(31), arm), /CLI hash/);
  assert.throws(() => assertCliSigningEquivalent(cli(arm), cli(arm).subarray(0, -1), arm), /segment bounds/);
  assert.throws(() => assertCliSigningEquivalent(cli(arm), Buffer.concat([cli(arm), Buffer.alloc(1)]), arm), /signature must occupy EOF/);
  assert.throws(() => assertCliSigningEquivalent(cli(arm), cli(arm), 'universal-apple-darwin'), /CLI hash/);
});
