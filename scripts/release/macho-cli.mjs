// Limited to the release CLI's thin, little-endian 64-bit executable profile.
// SDK loader.h/machine.h define these layouts/CPUs; 892's real ARM pair uses
// 16K LINKEDIT rounding. Intel uses I386_PGBYTES (4K); inspectApp still verifies
// the actual architecture and signature. This does not validate a signature.
const profiles = {
  'aarch64-apple-darwin': { cpu: 0x0100000c, page: 16384n },
  'x86_64-apple-darwin': { cpu: 0x01000007, page: 4096n },
};
const linkeditName = Buffer.from('__LINKEDIT\0\0\0\0\0\0');
const fail = reason => { throw new Error(`invalid CLI hash/signature equivalence: ${reason}`); };
function prefix(bytes, target) {
  const profile = profiles[target];
  if (!profile || !Buffer.isBuffer(bytes) || bytes.length < 32) fail('thin64 header');
  if (bytes.readUInt32LE(0) !== 0xfeedfacf || bytes.readUInt32LE(4) !== profile.cpu || bytes.readUInt32LE(12) !== 2) fail('thin64 executable/CPU');
  const count = bytes.readUInt32LE(16), size = bytes.readUInt32LE(20), end = 32 + size;
  if (!count || count > Math.floor(size / 8) || end > bytes.length) fail('load command count/bounds');
  const segments = []; let linkedit, signature, offset = 32;
  for (let i = 0; i < count; i++) {
    if (offset + 8 > end) fail('load command header');
    const cmd = bytes.readUInt32LE(offset), cmdsize = bytes.readUInt32LE(offset + 4);
    if (cmdsize < 8 || cmdsize % 8 || offset + cmdsize > end) fail('load command size');
    if (cmd === 0x19) {
      if (cmdsize < 72 || cmdsize !== 72 + 80 * bytes.readUInt32LE(offset + 64)) fail('segment sections/size');
      const vmaddr = bytes.readBigUInt64LE(offset + 24), vmsize = bytes.readBigUInt64LE(offset + 32);
      const fileoff = bytes.readBigUInt64LE(offset + 40), filesize = bytes.readBigUInt64LE(offset + 48);
      if (fileoff + filesize > BigInt(bytes.length) || vmaddr + vmsize > 0xffffffffffffffffn) fail('segment bounds');
      const segment = { offset, fileoff, filesize, vmsize }; segments.push(segment);
      if (bytes.subarray(offset + 8, offset + 24).equals(linkeditName)) {
        if (linkedit || cmdsize !== 72) fail('unique sectionless LINKEDIT');
        linkedit = segment;
      }
    } else if (cmd === 0x1d) {
      if (signature || cmdsize !== 16) fail('unique code signature');
      signature = { offset, start: bytes.readUInt32LE(offset + 8), size: bytes.readUInt32LE(offset + 12) };
    }
    offset += cmdsize;
  }
  if (offset !== end || !linkedit || !signature) fail('load command end/required commands');
  if (!signature.size || signature.start < end || signature.start + signature.size !== bytes.length) fail('signature must occupy EOF');
  const length = BigInt(bytes.length), start = BigInt(signature.start);
  if (linkedit.fileoff < BigInt(end) || linkedit.fileoff > start || linkedit.filesize !== length - linkedit.fileoff ||
      linkedit.vmsize !== (linkedit.filesize + profile.page - 1n) / profile.page * profile.page) fail('LINKEDIT derived sizes');
  if (segments.some(s => s !== linkedit && s.filesize > 0n && s.fileoff + s.filesize > linkedit.fileoff)) fail('LINKEDIT must be last file-backed segment');
  const result = Buffer.from(bytes.subarray(0, signature.start));
  result.fill(0, linkedit.offset + 32, linkedit.offset + 40);
  result.fill(0, linkedit.offset + 48, linkedit.offset + 56);
  result.fill(0, signature.offset + 12, signature.offset + 16);
  return result;
}

export function assertCliSigningEquivalent(staged, bundled, target) {
  if (!prefix(staged, target).equals(prefix(bundled, target))) fail('non-signature bytes changed');
}
