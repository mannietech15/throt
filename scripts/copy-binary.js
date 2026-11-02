const fs = require('fs');
const path = require('path');

const rootDir = path.resolve(__dirname, '..');
const targetRelease = path.join(rootDir, 'target', 'release');
const destDir = path.join(rootDir, 'packages', 'node');

fs.mkdirSync(destDir, { recursive: true });

const candidates = [
  path.join(targetRelease, 'libthrot_napi.so'),
  path.join(targetRelease, 'libthrot_napi.dylib'),
  path.join(targetRelease, 'throt_napi.dll'),
  path.join(targetRelease, 'throt.node')
];

let copied = false;
for (const cand of candidates) {
  if (fs.existsSync(cand)) {
    const dest = path.join(destDir, 'throt.node');
    fs.copyFileSync(cand, dest);
    console.log(`Copied native binary ${cand} -> ${dest}`);
    copied = true;
    break;
  }
}

if (!copied) {
  console.error('Error: native binary not found in target/release. Run `cargo build -p throt-napi --release` first.');
  process.exit(1);
}
