#!/usr/bin/env node
/**
 * One-step Vitest/Jest coverage for ts-crap.
 * Usage: node coverage-run.mjs [--framework vitest|jest|auto] [--provider istanbul|v8]
 *          [--format json|lcov] [--root .] [--output FILE] [--config FILE]
 *          [--scan] [--scanner ts-crap] [-- EXTRA FRAMEWORK ARGS]
 * Runs the framework inside ROOT through import-coverage.mjs with a fresh private
 * run directory, then prints (or with --scan runs) the matching scanner command.
 * Framework configuration is detected by filename only and is never evaluated.
 */
import { spawn } from 'node:child_process';
import { existsSync, readFileSync, realpathSync } from 'node:fs';
import * as fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseUniqueJson } from './strict-json.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const USAGE = 'Usage: node coverage-run.mjs [--framework vitest|jest|auto] [--provider istanbul|v8] '
  + '[--format json|lcov] [--root DIR] [--output FILE] [--config FILE] [--scan] [--scanner PATH] '
  + '[-- EXTRA FRAMEWORK ARGS]';
const VALUES = {
  '--framework': ['vitest', 'jest', 'auto'],
  '--provider': ['istanbul', 'v8'],
  '--format': ['json', 'lcov'],
};
const FLAGS = ['--scan'];
const OPTIONS = ['--framework', '--provider', '--format', '--root', '--output', '--config', '--scanner'];

export const CONFIG_NAMES = {
  vitest: ['vitest.config.ts', 'vitest.config.mts', 'vitest.config.cts',
    'vitest.config.js', 'vitest.config.mjs', 'vitest.config.cjs'],
  jest: ['jest.config.ts', 'jest.config.mts', 'jest.config.cts', 'jest.config.js',
    'jest.config.mjs', 'jest.config.cjs', 'jest.config.json'],
};

export class UsageError extends Error {}

export function parseArguments(argv) {
  const separator = argv.indexOf('--');
  const own = separator < 0 ? argv : argv.slice(0, separator);
  const options = { framework: 'auto', format: 'json', root: '.', scan: false,
    extra: separator < 0 ? [] : argv.slice(separator + 1) };
  const seen = new Set();
  for (let i = 0; i < own.length; i++) {
    const name = own[i];
    if (seen.has(name)) throw new UsageError(`Duplicate option ${name}\n${USAGE}`);
    seen.add(name);
    if (FLAGS.includes(name)) { options.scan = true; continue; }
    if (!OPTIONS.includes(name)) throw new UsageError(`Unknown option ${JSON.stringify(name)}\n${USAGE}`);
    const value = own[++i];
    if (value === undefined || value === '' || value.startsWith('--')) throw new UsageError(`Missing value for ${name}\n${USAGE}`);
    if (VALUES[name] && !VALUES[name].includes(value)) {
      throw new UsageError(`Invalid ${name} ${JSON.stringify(value)}; expected ${VALUES[name].join('|')}`);
    }
    options[name.slice(2)] = value;
  }
  if (options.scanner !== undefined && !options.scan) throw new UsageError('--scanner requires --scan');
  return options;
}

/** Detect the framework from configuration filenames and package.json only. */
export function detectFramework(root) {
  const found = new Set();
  const files = [];
  for (const [framework, names] of Object.entries(CONFIG_NAMES)) {
    for (const name of names) {
      if (existsSync(path.join(root, name))) { found.add(framework); files.push(name); }
    }
  }
  const manifest = path.join(root, 'package.json');
  if (existsSync(manifest)) {
    let json;
    try { json = parseUniqueJson(readFileSync(manifest, 'utf8')); }
    catch (error) { throw new Error(`Cannot read ${manifest} for framework detection: ${error.message}`); }
    if (json && typeof json === 'object' && Object.hasOwn(json, 'jest')) {
      found.add('jest'); files.push('package.json "jest" key');
    }
  }
  if (found.size === 1) return [...found][0];
  if (found.size > 1) {
    throw new UsageError(`Both Vitest and Jest configuration found (${files.join(', ')}); pass --framework vitest or --framework jest`);
  }
  throw new UsageError(`No Vitest or Jest configuration found in ${root} (looked for vitest.config.*, jest.config.* and a "jest" key in package.json); pass --framework vitest or --framework jest`);
}

/** Find an installed package from ROOT upwards, as Node resolution would for hoisted workspaces. */
export function findPackage(root, name) {
  for (let directory = root; ; directory = path.dirname(directory)) {
    const manifest = path.join(directory, 'node_modules', name, 'package.json');
    if (existsSync(manifest)) return { directory: path.dirname(manifest), manifest: parseUniqueJson(readFileSync(manifest, 'utf8')) };
    if (path.dirname(directory) === directory) return null;
  }
}

const INSTALL = {
  vitest: 'npm install --save-dev vitest @vitest/coverage-v8',
  jest: 'npm install --save-dev jest',
};

export function frameworkBinary(root, framework) {
  const found = findPackage(root, framework);
  if (!found) {
    throw new Error(`Cannot find ${framework} installed in ${root} or its parent directories; install it first (${INSTALL[framework]}) or run from the project that depends on it`);
  }
  const bin = typeof found.manifest.bin === 'string' ? found.manifest.bin : found.manifest.bin?.[framework];
  if (typeof bin !== 'string') throw new Error(`Installed ${framework} package declares no ${framework} executable: ${found.directory}`);
  return { path: path.resolve(found.directory, bin), version: found.manifest.version };
}

/** Framework arguments that select the reporter and its fresh directory. */
export function frameworkArguments(framework, { provider, format, directory, extra = [] }) {
  if (framework === 'vitest') {
    return ['run', '--coverage', `--coverage.reporter=${format === 'json' ? 'json' : 'lcovonly'}`,
      `--coverage.reportsDirectory=${directory}`,
      ...(provider ? [`--coverage.provider=${provider}`] : []), ...extra];
  }
  return ['--coverage', `--coverageReporters=${format === 'json' ? 'json' : 'lcovonly'}`,
    `--coverageDirectory=${directory}`,
    ...(provider ? [`--coverageProvider=${provider === 'istanbul' ? 'babel' : 'v8'}`] : []), ...extra];
}

export const reportName = format => format === 'json' ? 'coverage-final.json' : 'lcov.info';

/** The exact helper version: bundled version.json, or the checkout's Cargo.toml. */
export function helperVersion(directory = here) {
  const metadata = path.join(directory, 'version.json');
  if (existsSync(metadata)) return parseUniqueJson(readFileSync(metadata, 'utf8')).scannerVersion;
  const manifest = path.join(directory, '..', 'Cargo.toml');
  if (!existsSync(manifest)) return null;
  let section = '';
  for (const line of readFileSync(manifest, 'utf8').split(/\r?\n/)) {
    const header = /^\s*\[([^\]]+)\]\s*$/.exec(line);
    if (header) { section = header[1].trim(); continue; }
    const version = /^version\s*=\s*"([^"]+)"/.exec(line);
    if (section === 'package' && version) return version[1];
  }
  return null;
}

/** Quote for the platform's usual shell: POSIX sh, or cmd.exe/PowerShell on Windows. */
export function shellQuote(value, platform = process.platform) {
  if (/^[\w@%+=:,./\\-]+$/.test(value)) return value;
  return platform === 'win32' ? `"${value}"` : `'${value.replaceAll("'", "'\\''")}'`;
}

export function scannerArguments({ root, output, config }) {
  return ['--root', root, '--coverage', output, '--require-coverage', ...(config ? ['--config', config] : [])];
}

function run(command, args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: 'inherit', ...options });
    child.on('error', reject);
    child.on('exit', (code, signal) => resolve(signal ? 2 : code));
  });
}

function capture(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: ['ignore', 'pipe', 'inherit'] });
    let stdout = '';
    child.stdout.on('data', chunk => { stdout += chunk; });
    child.on('error', reject);
    // 'close' waits for stdout to drain; 'exit' can fire before the version is read.
    child.on('close', code => resolve({ code, stdout }));
  });
}

async function checkScanner(scanner) {
  const expected = helperVersion();
  if (!expected) throw new Error('Cannot determine the helper version (missing version.json); reinstall the complete helper bundle');
  let result;
  try { result = await capture(scanner, ['--version']); }
  catch (error) {
    if (error.code === 'ENOENT') throw new Error(`Scanner not found: ${scanner}; install ts-crap ${expected} or pass --scanner PATH`);
    if (error.code === 'EINVAL') throw new Error(`Cannot execute scanner ${scanner}; pass --scanner with the ts-crap executable itself, not a .cmd/.bat shim`);
    throw error;
  }
  const actual = result.stdout.trim();
  if (result.code !== 0 || actual !== `ts-crap ${expected}`) {
    throw new Error(`Helper/scanner version mismatch: helpers are ${expected} but ${scanner} reports ${JSON.stringify(actual)}; use the same exact release for both`);
  }
}

async function main() {
  const options = parseArguments(process.argv.slice(2));
  const root = await fs.realpath(options.root).catch(error => {
    throw new Error(`Cannot resolve --root ${options.root}: ${error.message}`);
  });
  const framework = options.framework === 'auto' ? detectFramework(root) : options.framework;
  const binary = frameworkBinary(root, framework);
  if (framework === 'vitest') {
    // Vitest offers to install a missing provider and edit package.json; refuse first.
    const providers = options.provider ? [options.provider] : ['v8', 'istanbul'];
    if (!providers.some(provider => findPackage(root, `@vitest/coverage-${provider}`))) {
      const wanted = options.provider ?? 'v8';
      throw new Error(`${options.provider ? `--provider ${wanted}` : 'Vitest coverage'} needs @vitest/coverage-${wanted}; install it with npm install --save-dev @vitest/coverage-${wanted}@${binary.version}`);
    }
  }
  const scanner = options.scanner ?? 'ts-crap';
  // Fail before running tests rather than after a long run with an unusable scanner.
  if (options.scan) await checkScanner(scanner);
  // A new private directory guarantees fresh reporter and native destinations.
  const directory = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'ts-crap-run-')));
  const input = path.join(directory, reportName(options.format));
  const output = options.output ? path.resolve(options.output) : path.join(directory, 'native-coverage.json');
  const config = options.config ? path.resolve(options.config) : undefined;
  console.error(`ts-crap coverage-run: ${framework} ${binary.version ?? ''} (${options.provider ?? 'configured provider'}, ${options.format}) in ${root}`);
  const status = await run(process.execPath, [
    path.join(here, 'import-coverage.mjs'), '--root', root,
    '--format', options.format === 'json' ? 'istanbul' : 'lcov', '--input', input, '--output', output,
    ...(config ? ['--config', config] : []), '--', process.execPath, binary.path,
    ...frameworkArguments(framework, { provider: options.provider, format: options.format, directory, extra: options.extra }),
  ]);
  if (status !== 0) {
    console.error(`ts-crap coverage-run: no coverage published; reporter output (if any) is in ${directory}`);
    process.exitCode = status;
    return;
  }
  if (options.output) await fs.rm(directory, { recursive: true, force: true });
  const scan = scannerArguments({ root, output, config });
  console.error(`ts-crap coverage-run: native coverage written to ${output}`);
  if (!options.scan) {
    console.log(['ts-crap', ...scan].map(shellQuote).join(' '));
    return;
  }
  process.exitCode = await run(scanner, scan);
}

const invoked = process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url));
if (invoked) {
  main().catch(error => {
    console.error(`ts-crap coverage-run: ${error.message}`);
    process.exitCode = 2;
  });
}
