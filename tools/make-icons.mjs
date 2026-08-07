// Generates the Torch flame artwork used for the app icon and the tray icons.
// Run: node tools/make-icons.mjs
// Then: npx @tauri-apps/cli@2 icon src-tauri/icons/source.png -o src-tauri/icons
import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "icons");
mkdirSync(OUT, { recursive: true });

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(buf) {
  let c = 0xffffffff;
  for (const byte of buf) c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

function encodePng(width, height, rgba) {
  const stride = width * 4;
  const raw = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (stride + 1)] = 0;
    rgba.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

const lerp = (a, b, t) => a + (b - a) * t;
const mix = (a, b, t) => [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)];

// Flame silhouette in normalised coordinates: x in [-1, 1], y from 0 (base) to 1 (tip).
function insideFlame(x, y, scale, lean) {
  if (y < 0 || y > scale) return false;
  const t = y / scale;
  const sway = lean * Math.sin(t * Math.PI) * 0.09 * scale;
  const width =
    t < 0.42
      ? 0.62 * scale * Math.sqrt(Math.max(0, 1 - Math.pow((0.42 - t) / 0.42, 2)))
      : 0.62 * scale * Math.pow(Math.max(0, 1 - (t - 0.42) / 0.58), 0.72);
  return Math.abs(x - sway) <= width;
}

function render(size, { grayscale }) {
  const rgba = Buffer.alloc(size * size * 4);
  const ss = 3; // supersampling factor
  const outerLow = [255, 61, 0];
  const outerHigh = [255, 176, 32];
  const innerLow = [255, 196, 64];
  const innerHigh = [255, 249, 214];

  for (let py = 0; py < size; py++) {
    for (let px = 0; px < size; px++) {
      let acc = [0, 0, 0, 0];
      for (let sy = 0; sy < ss; sy++) {
        for (let sx = 0; sx < ss; sx++) {
          const fx = ((px + (sx + 0.5) / ss) / size) * 2 - 1;
          const fy = 1 - (py + (sy + 0.5) / ss) / size;
          const x = (fx - 0.02) / 0.9;
          const y = (fy - 0.05) / 0.9;

          const outer = insideFlame(x, y, 1, 1);
          const inner = insideFlame((x - 0.01) / 0.5, (y - 0.02) / 0.5, 1, 0.7);
          if (!outer) continue;

          let color = inner
            ? mix(innerLow, innerHigh, Math.min(1, y / 0.55))
            : mix(outerLow, outerHigh, Math.min(1, Math.pow(y, 0.8)));
          if (grayscale) {
            const l = 0.3 * color[0] + 0.59 * color[1] + 0.11 * color[2];
            const g = lerp(l, 150, 0.35);
            color = [g, g, g];
          }
          acc = [acc[0] + color[0], acc[1] + color[1], acc[2] + color[2], acc[3] + 255];
        }
      }
      const samples = ss * ss;
      const alpha = acc[3] / samples;
      const i = (py * size + px) * 4;
      if (alpha > 0) {
        rgba[i] = Math.round(acc[0] / (acc[3] / 255));
        rgba[i + 1] = Math.round(acc[1] / (acc[3] / 255));
        rgba[i + 2] = Math.round(acc[2] / (acc[3] / 255));
        rgba[i + 3] = Math.round(alpha);
      }
    }
  }
  return encodePng(size, size, rgba);
}

writeFileSync(join(OUT, "source.png"), render(1024, { grayscale: false }));
writeFileSync(join(OUT, "tray-on.png"), render(64, { grayscale: false }));
writeFileSync(join(OUT, "tray-off.png"), render(64, { grayscale: true }));
console.log("icons written to", OUT);
