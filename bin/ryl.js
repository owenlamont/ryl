#!/usr/bin/env node

const path = require('path');
const { spawnSync } = require('child_process');

const pkg = require('../package.json');
const platforms = require('../npm-platforms.json');
const binName = process.platform === 'win32' ? 'ryl.exe' : 'ryl';
const localBinPath = path.join(__dirname, binName);

async function run() {
  if (process.env.RYL_BINARY_PATH) {
    execute(process.env.RYL_BINARY_PATH);
    return;
  }

  if (exists(localBinPath)) {
    execute(localBinPath);
    return;
  }

  const platformPackage = resolvePlatformPackage();
  const platformPackageJson = resolveInstalledPackageJson(platformPackage.packageName);
  if (!platformPackageJson) {
    console.error(
      [
        `No installed npm platform package matches ${process.platform}/${process.arch}.`,
        `Expected optional dependency: ${platformPackage.packageName}`,
        'This package requires npm optionalDependencies; installs done with',
        '--omit=optional or npm_config_optional=false are not supported.',
        `Reinstall ${pkg.name} with optional dependencies enabled.`
      ].join('\n')
    );
    process.exit(1);
  }

  const packageRoot = path.dirname(platformPackageJson);
  const binaryPath = path.join(packageRoot, 'bin', platformPackage.binaryName);
  if (!exists(binaryPath)) {
    console.error(
      `Installed package ${platformPackage.packageName} is missing binary ${platformPackage.binaryName}.`
    );
    process.exit(1);
  }

  execute(binaryPath);
}

function exists(candidatePath) {
  try {
    require('fs').accessSync(candidatePath);
    return true;
  } catch {
    return false;
  }
}

function resolvePlatformPackage() {
  for (const platform of platforms.platforms) {
    if (platform.os.includes(process.platform) && platform.cpu.includes(process.arch)) {
      return platform;
    }
  }

  console.error(`Unsupported platform/architecture: ${process.platform}/${process.arch}`);
  process.exit(1);
}

function resolveInstalledPackageJson(packageName) {
  try {
    return require.resolve(`${packageName}/package.json`);
  } catch {
    return null;
  }
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
