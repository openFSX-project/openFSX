// Lossless converter for the bounded P6 RGB snapshots written by our renderer.
import { readFile, writeFile } from 'node:fs/promises';
import { deflateSync } from 'node:zlib';
const [input, output] = process.argv.slice(2);
if (!input || !output) throw new Error('Usage: node scripts/ppm-to-png.mjs input.ppm output.png');
const ppm = await readFile(input);
const header = ppm.subarray(0, 64).toString('ascii').match(/^P6\n(\d+) (\d+)\n255\n/);
if (!header) throw new Error('Expected an openFSX P6 RGB snapshot');
const width = Number(header[1]), height = Number(header[2]);
if (width < 1 || height < 1 || width > 4096 || height > 4096 || ppm.length !== header[0].length + width * height * 3) {
  throw new Error('Invalid snapshot dimensions or RGB payload size');
}
function chunk(tag, data) {
  const type = Buffer.from(tag), size = Buffer.alloc(4), checksum = Buffer.alloc(4);
  size.writeUInt32BE(data.length);
  let crc = 0xffffffff;
  for (const byte of Buffer.concat([type, data])) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0);
  }
  checksum.writeUInt32BE((crc ^ 0xffffffff) >>> 0);
  return Buffer.concat([size, type, data, checksum]);
}
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(width); ihdr.writeUInt32BE(height, 4); ihdr[8] = 8; ihdr[9] = 2;
const rows = Buffer.alloc(height * (width * 3 + 1));
for (let y = 0; y < height; y++) {
  ppm.copy(rows, y * (width * 3 + 1) + 1, header[0].length + y * width * 3, header[0].length + (y + 1) * width * 3);
}
await writeFile(output, Buffer.concat([
  Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
  chunk('IHDR', ihdr), chunk('IDAT', deflateSync(rows)), chunk('IEND', Buffer.alloc(0)),
]));
console.log(`PNG: ${output} (${width}x${height})`);
