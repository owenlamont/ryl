#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const https = require('https');
const { spawnSync } = require('child_process');

const pkg = require('../package.json');
const version = pkg.version;
const binName = process.platform === 'win32' ? 'ryl.exe' : 'ryl';
const binDir = path.join(__dirname, '..', 'bin');
const binPath = path.join(binDir, binName);

const PLATFORMS = {
  darwin: {
    arm64: 'aarch64-apple-darwin'
  },
  linux: {
    x64: 'x86_64-unknown-linux-musl',
    arm64: 'aarch64-unknown-linux-musl',
    arm: 'armv7-unknown-linux-gnueabihf'
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
        download(response.headers.location, dest).then(resolve).catch(reject);
        return;
      }
      if (response.statusCode !== 200) {
        reject(new Error(`Failed to download binary: ${response.statusCode}`));
        return;
      }
      response.pipe(file);
      file.on('finish', () => {
        file.close();
        resolve();
      });
    }).on('error', (err) => {
      fs.unlink(dest, () => reject(err));
    });
  });
}

async function install() {
  const binaryAsset = getBinaryName();
  if (!binaryAsset) {
    console.error(`Unsupported platform/architecture: ${process.platform}/${process.arch}`);
    process.exit(1);
  }

  const url = `https://github.com/owenlamont/ryl/releases/download/v${version}/${binaryAsset}`;
  const archivePath = path.join(binDir, binaryAsset);

  console.log(`Downloading ryl v${version} for ${process.platform}/${process.arch}...`);
  try {
    await download(url, archivePath);

    // Simple extraction logic depending on platform
    if (process.platform === 'win32') {
      // For Windows, we'd ideally use a library like 'unzipper' or 'adm-zip'
      // but to keep it zero-dependency, we can try using the system's tar/powershell
      spawnSync('powershell.exe', ['-Command', `Expand-Archive -Path "${archivePath}" -DestinationPath "${binDir}" -Force`], { stdio: 'inherit' });
    } else {
      spawnSync('tar', ['-xzf', archivePath, '-C', binDir], { stdio: 'inherit' });
    }

    fs.unlinkSync(archivePath);
    if (process.platform !== 'win32') {
      fs.chmodSync(binPath, 0o755);
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
  if (!fs.existsSync(binPath)) {
    await install();
  }

  const result = spawnSync(binPath, process.argv.slice(2), {
    stdio: 'inherit',
    windowsHide: true
  });

  process.exit(result.status || 0);
}

run();
