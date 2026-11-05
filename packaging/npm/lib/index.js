'use strict';
/**
 * Locates and runs the prebuilt ts-crap scanner from this package's matching
 * platform package. There is no install script and nothing is downloaded: the
 * package manager installs exactly one optional platform package by os/cpu.
 */
const { spawn } = require('node:child_process');
const path = require('node:path');

const MAIN = 'typescript-crap';
// process.platform-process.arch -> platform package. Linux uses the static musl
// build, so one package per CPU serves glibc and musl systems alike.
const PLATFORMS = {
  'linux-x64': 'typescript-crap-linux-x64',
  'linux-arm64': 'typescript-crap-linux-arm64',
  'darwin-x64': 'typescript-crap-darwin-x64',
  'darwin-arm64': 'typescript-crap-darwin-arm64',
  'win32-x64': 'typescript-crap-win32-x64',
};

class LauncherError extends Error {}

function version() {
  return require('../package.json').version;
}

function platformPackage(platform = process.platform, arch = process.arch) {
  const name = PLATFORMS[`${platform}-${arch}`];
  if (!name) {
    throw new LauncherError(`${MAIN}: no prebuilt ts-crap for ${platform}-${arch}; supported platforms are `
      + `${Object.keys(PLATFORMS).join(', ')}. See https://github.com/RuiCostafrg/typescript-crap#install to build from source`);
  }
  return name;
}

/** Absolute path of the installed native scanner; throws a LauncherError with the fix. */
function binaryPath(options = {}) {
  const platform = options.platform ?? process.platform;
  const name = platformPackage(platform, options.arch ?? process.arch);
  const resolve = options.resolve ?? (request => require.resolve(request));
  let manifest;
  try {
    manifest = resolve(`${name}/package.json`);
  } catch {
    throw new LauncherError(`${MAIN}: the platform package ${name} is not installed. It is an optional dependency, `
      + 'so installs that omit optional dependencies (--omit=optional, --no-optional) or copy node_modules between '
      + `platforms skip it. Reinstall ${MAIN} on this machine with optional dependencies enabled.`);
  }
  const installed = (options.readVersion ?? (file => require(file).version))(manifest);
  const expected = options.version ?? version();
  if (installed !== expected) {
    throw new LauncherError(`${MAIN}: ${name} is version ${installed} but ${MAIN} is ${expected}; `
      + `reinstall so both are ${expected}`);
  }
  return path.join(path.dirname(manifest), 'bin', platform === 'win32' ? 'ts-crap.exe' : 'ts-crap');
}

const FORWARDED = ['SIGINT', 'SIGTERM', 'SIGHUP'];

/**
 * Run a child with inherited stdio, forwarding termination signals, and settle
 * with its exit status. A child stopped by a signal resolves to { signal }.
 */
function run(command, args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: 'inherit', windowsHide: true, ...options });
    const forwards = FORWARDED.map(signal => [signal, () => child.kill(signal)]);
    for (const [signal, forward] of forwards) process.on(signal, forward);
    const stop = () => { for (const [signal, forward] of forwards) process.off(signal, forward); };
    child.on('error', error => { stop(); reject(error); });
    child.on('exit', (code, signal) => { stop(); resolve(signal ? { signal } : { code }); });
  });
}

/** Mirror the child's result: same exit code, or the same terminating signal. */
function finish(result) {
  if (result.signal) {
    process.kill(process.pid, result.signal);
    return;
  }
  process.exitCode = result.code;
}

function fail(error) {
  // The scanner reports usage and runtime errors with exit status 2.
  let message = error instanceof LauncherError ? error.message : `${MAIN}: ${error.message}`;
  if (error.code === 'EACCES') message += `; the executable lost its permission bits, so reinstall ${MAIN} on this machine`;
  console.error(message);
  process.exitCode = 2;
}

/** Run the native scanner with ARGS, exactly as the standalone `ts-crap` binary. */
function runScanner(args) {
  let binary;
  try { binary = binaryPath(); } catch (error) { fail(error); return Promise.resolve(); }
  return run(binary, args).then(finish, fail);
}

const COVERAGE_RUN = path.join(__dirname, '..', 'coverage', 'coverage-run.mjs');

/**
 * Arguments for the bundled one-step coverage command. With --scan and no
 * --scanner, scan with this package's own native binary: it is the exact
 * matching version, and spawning it avoids Windows .cmd shims on PATH.
 */
function coverageArguments(args, scanner) {
  const separator = args.indexOf('--');
  const own = separator < 0 ? args : args.slice(0, separator);
  if (!own.includes('--scan') || own.includes('--scanner')) return [...args];
  return [...own, '--scanner', scanner(), ...(separator < 0 ? [] : args.slice(separator))];
}

// Yarn Plug'n'Play's runtime, which `yarn run` adds to NODE_OPTIONS.
const PNP_OPTION = /(?:^|\s)(?:--require|-r|--experimental-loader|--loader|--import)(?:\s+|=)("[^"]*"|'[^']*'|\S+)/g;

/**
 * NODE_OPTIONS without Yarn Plug'n'Play's runtime. The helpers and the test
 * framework they find run from node_modules on disk; the PnP filesystem patch
 * would otherwise follow them into every child process and reject some calls.
 */
function withoutPnp(options) {
  if (!options) return options;
  return options.replace(PNP_OPTION, (match, value) => (/\.pnp\.(?:c?js|loader\.mjs)["']?$/.test(value) ? '' : match)).trim();
}

function nodeMajor(versions = process.versions) {
  return Number(versions.node.split('.')[0]);
}

/** `typescript-crap coverage ARGS`: the helpers' coverage-run.mjs with this package's scanner. */
function runCoverage(args, options = {}) {
  const versions = options.versions ?? process.versions;
  if (nodeMajor(versions) < 24) {
    fail(new LauncherError(`${MAIN} coverage: the coverage helpers need Node.js 24 or later (this is ${versions.node}); `
      + 'the scanner itself (ts-crap) runs on any supported Node.js'));
    return Promise.resolve();
  }
  let forwarded;
  try { forwarded = coverageArguments(args, () => binaryPath()); } catch (error) { fail(error); return Promise.resolve(); }
  // The printed follow-up command runs this same exact version through npx.
  const env = { ...process.env };
  env.TS_CRAP_SCANNER_COMMAND ??= JSON.stringify(['npx', `${MAIN}@${version()}`]);
  if (env.NODE_OPTIONS) env.NODE_OPTIONS = withoutPnp(env.NODE_OPTIONS);
  return run(process.execPath, [COVERAGE_RUN, ...forwarded], { env }).then(finish, fail);
}

module.exports = {
  COVERAGE_RUN, FORWARDED, LauncherError, PLATFORMS, binaryPath, coverageArguments, finish, fail, nodeMajor,
  platformPackage, run, runCoverage, runScanner, version, withoutPnp,
};
