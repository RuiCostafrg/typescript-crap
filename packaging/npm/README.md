# typescript-crap

Source-aware CRAP scores for TypeScript functions: the `ts-crap` quality gate
and its Vitest/Jest coverage helpers, from
[RuiCostafrg/typescript-crap](https://github.com/RuiCostafrg/typescript-crap).

```sh
npm install --save-dev typescript-crap
npx ts-crap --root . --threshold 30
```

The package installs a prebuilt native scanner for Linux (x64, arm64; static,
so glibc and musl both work), macOS (x64, arm64) and Windows (x64). The
executable comes from a platform package (`typescript-crap-linux-x64` and so
on), which your package manager selects by `os` and `cpu` and fetches like
any other dependency. There is no install script and no download outside the
package manager, so installing works with
`--ignore-scripts`, pnpm, Yarn (including Plug'n'Play), Bun, offline caches and
registry mirrors. Do not omit optional dependencies (`--omit=optional`): the
platform packages are optional dependencies.

## Commands

- `ts-crap ARGS`: the scanner. Arguments, output and exit codes match the
  standalone binary: 0 pass, 1 quality gate failed, 2 error.
- `typescript-crap ARGS`: the same scanner, so one-off runs work with
  `npx typescript-crap@VERSION ARGS`.
- `typescript-crap coverage ARGS`: the one-step Vitest/Jest coverage command
  (Node.js 24 or later). It runs your tests with coverage, imports the result
  and prints the scanner command; with `--scan` it runs the scan with this
  package's scanner:

  ```sh
  npx typescript-crap coverage --framework vitest --scan
  ```

  The coverage command runs your test framework from `node_modules`, so it
  does not support Yarn Plug'n'Play: set `nodeLinker: node-modules` in
  `.yarnrc.yml` first. The scanner itself works under Plug'n'Play.

The other coverage helpers are in `node_modules/typescript-crap/coverage/`,
for example `node node_modules/typescript-crap/coverage/coverage.mjs`. They are
byte-identical to the release's `ts-crap-coverage-helpers.zip`, at the same
version as the scanner. See the
[coverage helper guide](https://github.com/RuiCostafrg/typescript-crap/blob/main/docs/COVERAGE_HELPERS.md).

## Verification

Every package is published from the repository's release workflow with npm
trusted publishing and provenance. Each platform package's executable is
checked, before publishing, to be byte-identical to the attested GitHub release
archive. Check the registry signatures and provenance of an install with
`npm audit signatures`. See the
[security policy](https://github.com/RuiCostafrg/typescript-crap/blob/main/SECURITY.md).

MIT licensed.
