const path = require('path');
const fs = require('fs');

function loadBinding() {
  const localFile = path.join(__dirname, 'throt.node');
  if (fs.existsSync(localFile)) {
    return require(localFile);
  }

  const { platform, arch } = process;
  let nativePkg = '';

  switch (platform) {
    case 'linux':
      if (arch === 'x64') {
        nativePkg = '@throt/node-linux-x64-gnu';
      } else if (arch === 'arm64') {
        nativePkg = '@throt/node-linux-arm64-gnu';
      }
      break;
    case 'darwin':
      if (arch === 'x64') {
        nativePkg = '@throt/node-darwin-x64';
      } else if (arch === 'arm64') {
        nativePkg = '@throt/node-darwin-arm64';
      }
      break;
    case 'win32':
      if (arch === 'x64') {
        nativePkg = '@throt/node-win32-x64-msvc';
      }
      break;
    default:
      break;
  }

  if (nativePkg) {
    try {
      return require(nativePkg);
    } catch (e) {
      // fallback
    }
  }

  throw new Error(
    `Failed to load native THROT binary. Attempted local: ${localFile}, optional package: ${nativePkg}. ` +
    `Ensure a supported platform/architecture (${platform}-${arch}) is running.`
  );
}

const binding = loadBinding();

const ThrotEngine = binding.ThrotEngine;
const { expressMiddleware } = require('./express');
const { fastifyRateLimit } = require('./fastify');

function createEngine(config) {
  return ThrotEngine.create(config);
}

module.exports = {
  ThrotEngine,
  createEngine,
  expressMiddleware,
  fastifyRateLimit,
};

module.exports.ThrotEngine = ThrotEngine;
module.exports.createEngine = createEngine;
module.exports.expressMiddleware = expressMiddleware;
module.exports.fastifyRateLimit = fastifyRateLimit;

