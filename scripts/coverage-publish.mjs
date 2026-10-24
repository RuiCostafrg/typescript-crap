import * as fs from 'node:fs/promises';
import path from 'node:path';

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
