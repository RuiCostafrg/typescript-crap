import * as fs from 'node:fs/promises';
import path from 'node:path';

/** Resolve existing ancestors so absent destinations through directory aliases compare correctly. */
async function location(filename) {
  try { return await fs.realpath(filename); }
  catch (error) {
    const parent = path.dirname(filename);
    if (error.code !== 'ENOENT' || parent === filename) throw error;
    return path.join(await location(parent), path.basename(filename));
  }
}

/** Reserve default and caller-selected policy paths without loading configuration. */
export async function protectConfigDestinations(root, destinations, config) {
  const protectedPaths = [path.join(root, 'ts-crap.json')];
  if (config !== undefined) protectedPaths.push(path.resolve(config));
  // Reserve basename case variants on every platform, including an absent
  // default config on case-insensitive macOS volumes.
  const key = filename => {
    const normalized = path.join(path.dirname(filename), path.basename(filename).toLowerCase());
    return process.platform === 'win32' ? normalized.toLowerCase() : normalized;
  };
  // Publication replaces directory entries, so a hardlink at another name is safe.
  // Reserve both the config entry and its resolved target for config symlinks.
  const entry = async filename => path.join(await location(path.dirname(filename)), path.basename(filename));
  const protectedLocations = new Set();
  for (const filename of protectedPaths) {
    protectedLocations.add(key(await entry(filename)));
    protectedLocations.add(key(await location(filename)));
  }
  for (const destination of destinations) {
    if (protectedLocations.has(key(await entry(destination)))) {
      throw new Error(`Coverage destination cannot overwrite project configuration: ${destination}`);
    }
  }
}

/** Stage a complete report beside its destination, then publish it in one step. */
export async function publishReport(
  output,
  contents,
  { replace = false, createParent = false, validate } = {},
) {
  if (createParent) await fs.mkdir(path.dirname(output), { recursive: true });
  // A random private sibling directory keeps staging on the destination filesystem.
  const directory = await fs.mkdtemp(path.join(path.dirname(output), '.ts-crap-coverage-'));
  const staging = path.join(directory, 'report.json');
  let ownsFile = false;
  try {
    const handle = await fs.open(staging, 'wx', 0o600);
    ownsFile = true;
    try {
      await handle.writeFile(contents);
      // Flush file contents before publication. This is not a power-loss guarantee:
      // directory-entry durability still depends on the filesystem.
      await handle.sync();
    } finally {
      await handle.close();
    }
    if (validate) await validate();
    if (replace) {
      await fs.rename(staging, output);
      ownsFile = false;
    } else {
      // Linking publishes without replacing anything a concurrent writer created.
      try {
        await fs.link(staging, output);
      } catch (error) {
        if (error.code !== 'EEXIST') throw error;
        throw new Error(`Coverage destination already exists: ${output}; use a fresh path`);
      }
    }
  } finally {
    if (ownsFile) await fs.rm(staging, { force: true });
    // Never recursively remove entries we did not create.
    await fs.rmdir(directory);
  }
}
