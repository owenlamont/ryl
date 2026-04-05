#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const https = require('https');
const os = require('os');
const { spawnSync } = require('child_process');

const pkg = require('../package.json');
const version = pkg.version;
const binName = process.platform === 'win32' ? 'ryl.exe' : 'ryl';

// Support user-writable cache directory to avoid EACCES in global installs
function getCacheDir() {
  if (process.platform === 'win32') {
    return process.env.LOCALAPPDATA || path.join(os.homedir(), 'AppData', 'Local');
  }
  if (process.platform === 'darwin') {
    return path.join(os.homedir(), 'Library', 'Caches');
  }
  return process.env.XDG_CACHE_HOME || path.join(os.homedir(), '.cache');
}

const rylCacheDir = path.join(getCacheDir(), 'ryl', version);
const cacheBinPath = path.join(rylCacheDir, binName);
const localBinPath = path.join(__dirname, binName);

const PLATFORMS = {
  darwin: {
    arm64: 'aarch64-apple-darwin'
  },
  linux: {
    x64: 'x86_64-unknown-linux-musl',
    arm64: 'aarch64-unknown-linux-musl',
    arm: 'armv7-unknown-linux-gnueabihf',
    ia32: 'i686-unknown-linux-gnu',
    ppc64: 'powerpc64le-unknown-linux-gnu',
    s390x: 's390x-unknown-linux-gnu'
  },
  win32: {
    x64: 'x86_64-pc-windows-msvc',
    arm64: 'aarch64-pc-windows-msvc'
  }
};

function getBinaryName() {
  const platform = PLATFORMS[process.platform];
  if (!platform) return null;
  const target = platform[process.arch];
  if (!target) return null;

  if (process.platform === 'win32') {
    return `ryl-${target}.zip`;
  }
  return `ryl-${target}.tar.gz`;
}

function download(url, dest) {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(dest);
    https.get(url, (response) => {
      if (response.statusCode === 302 || response.statusCode === 301) {
        file.close(() => {
          fs.unlink(dest, () => {
            download(response.headers.location, dest).then(resolve).catch(reject);
          });
        });
        return;
      }
      if (response.statusCode !== 200) {
        file.close(() => {
          fs.unlink(dest, () => {
            reject(new Error(`Failed to download binary: ${response.statusCode}`));
          });
        });
        return;
      }
      response.pipe(file);
      file.on('finish', () => {
        file.close();
        resolve();
      });
    }).on('error', (err) => {
      file.close(() => {
        fs.unlink(dest, () => reject(err));
      });
    });
  });
}

async function install() {
  const binaryAsset = getBinaryName();
  if (!binaryAsset) {
    console.error(`Unsupported platform/architecture: ${process.platform}/${process.arch}`);
    process.exit(1);
  }

  // Ensure cache directory exists
  try {
    fs.mkdirSync(rylCacheDir, { recursive: true });
  } catch (err) {
    console.error(`Error creating cache directory ${rylCacheDir}: ${err.message}`);
    process.exit(1);
  }

  const url = `https://github.com/owenlamont/ryl/releases/download/v${version}/${binaryAsset}`;
  const archivePath = path.join(rylCacheDir, binaryAsset);

  console.log(`Downloading ryl v${version} for ${process.platform}/${process.arch}...`);
  console.log(`Installing to: ${cacheBinPath}`);

  try {
    await download(url, archivePath);

    if (process.platform === 'win32') {
      spawnSync('powershell.exe', ['-Command', `Expand-Archive -Path "${archivePath}" -DestinationPath "${rylCacheDir}" -Force`], { stdio: 'inherit' });
    } else {
      spawnSync('tar', ['-xzf', archivePath, '-C', rylCacheDir], { stdio: 'inherit' });
    }

    fs.unlinkSync(archivePath);
    if (process.platform !== 'win32') {
      fs.chmodSync(cacheBinPath, 0o755);
    }
    console.log('Successfully installed ryl!');
  } catch (err) {
    console.error(`Error installing ryl binary: ${err.message}`);
    if (process.env.HTTPS_PROXY || process.env.https_proxy || process.env.HTTP_PROXY || process.env.http_proxy) {
      console.error('Note: You are using a proxy; please ensure your environment is configured correctly for Node.js https.get().');
    }
    process.exit(1);
  }
}

async function run() {
  // 1. Try Local Bin (for dev/local installs)
  if (fs.existsSync(localBinPath)) {
    execute(localBinPath);
    return;
  }

  // 2. Try Cache Bin
  if (fs.existsSync(cacheBinPath)) {
    execute(cacheBinPath);
    return;
  }

  // 3. Install and Execute
  await install();
  execute(cacheBinPath);
}

function execute(binPath) {
  const result = spawnSync(binPath, process.argv.slice(2), {
    stdio: 'inherit',
    windowsHide: true
  });

  if (result.error) {
    console.error(`Error executing binary at ${binPath}: ${result.error.message}`);
    process.exit(1);
  }

  if (result.status === null) {
    console.error('Binary execution failed (no exit status)');
    process.exit(1);
  }

  process.exit(result.status);
}

run();
