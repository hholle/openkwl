// Packs the web build for the SD card: each file under an 8.3 name in dist-sd/WWW, gzipped
// except the fonts (WOFF2 is compressed already). The firmware serves `/` → INDEX.HTM,
// `/<name>` → upper-cased 8.3 name, and long names (icons, /fonts/…) through the alias table
// in firmware/src/http.rs (`www_name`); with `Content-Encoding: gzip` for everything but
// `.WF2` (embedded-sdmmc has no long-file-name support).
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { gzipSync } from 'node:zlib';

const FILES = [
  ['index.html', 'INDEX.HTM'],
  ['app.js', 'APP.JS'],
  ['app.css', 'APP.CSS'],
  // Icons from public/ (KWL favicons); keep in sync with ALIASES in firmware/src/http.rs.
  ['favicon.ico', 'FAVICON.ICO'],
  ['favicon.svg', 'FAVICON.SVG'],
  ['apple-touch-icon.png', 'APPLETCH.PNG'],
  ['icon-192.png', 'ICON192.PNG'],
  ['icon-512.png', 'ICON512.PNG'],
  ['icon-maskable-512.png', 'ICONM512.PNG'],
  ['site.webmanifest', 'MANIFEST.WEB'],
  // Geist, served as /fonts/… (SIL Open Font License: OFL.txt ships beside the fonts).
  ['fonts/Geist-Variable.woff2', 'GEISTVAR.WF2', { raw: true }],
  ['fonts/GeistMono-Regular.woff2', 'GEISTMNO.WF2', { raw: true }],
  ['fonts/OFL.txt', 'OFL.TXT'],
];
const out = new URL('../../dist-sd/WWW/', import.meta.url);
rmSync(new URL('../', out), { recursive: true, force: true });
mkdirSync(out, { recursive: true });

const html = readFileSync(new URL('../dist/index.html', import.meta.url), 'utf8');
for (const ref of ['/app.js', '/app.css']) {
  if (!html.includes(ref)) throw new Error(`index.html does not reference ${ref}; check vite.config.ts output names`);
}

let total = 0;
for (const [src, dst, { raw = false } = {}] of FILES) {
  if (!/^[A-Z0-9]{1,8}\.[A-Z0-9]{1,3}$/.test(dst)) throw new Error(`${dst} is not an 8.3 name`);
  const data = readFileSync(new URL(`../dist/${src}`, import.meta.url));
  const gz = raw ? data : gzipSync(data, { level: 9 });
  writeFileSync(new URL(dst, out), gz);
  total += gz.length;
  console.log(`${dst.padEnd(10)} ${String(Math.round(data.length / 1024)).padStart(4)} KB → ${String(Math.round(gz.length / 1024)).padStart(4)} KB${raw ? ' (stored)' : ' gzip'}`);
}
console.log(`total      ${String(Math.round(total / 1024)).padStart(12)} KB in ${new URL('.', out).pathname}`);
