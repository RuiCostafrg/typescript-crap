# Independent adoption validation

Three independent repositories are pinned in
[the project manifest](../tests/adoption/projects.json). The harness fetches their
actual Git commits with sparse checkout, leaves tracked source unchanged, runs
targeted real Vitest/Istanbul tests, imports JSON coverage, and scans twice to
check deterministic output. This is integration validation against upstream
source, not an endorsement by those projects or their complete test suites.

## Reproduce

Use Node 24+, Git, Python 3 on POSIX, and the release scanner build:

```sh
npm ci --ignore-scripts --no-audit --no-fund --prefix tests/tooling
cargo build --locked --release
python3 scripts/measure-adoption.py --scanner target/release/ts-crap \
  --build-profile release --workspace /tmp/independent-projects \
  --output target/adoption.json
```

The script fetches exact commits rather than a moving branch/tag. It records
scanner version/SHA-256, Node/Vitest versions, selected paths, gate exits, missing
evidence, highest scores, elapsed time, and per-process peak RSS. CI reruns it on
Ubuntu 26.04/Node 24 and uploads its own measurements. Runtime measurements vary
by machine and are observations, not pass/fail performance budgets.

## Measured projects

The [committed measurement](measurements/adoption-v1.json) was collected on
2026-10-26 using an optimized development scanner (package version 0.4.0),
Node 26.3.1 and Vitest/Istanbul 5.0.5 on an Android aarch64/Ubuntu 26.04 Termux
proot environment. Its binary hash identifies this unreleased build; it is not a
measurement of the published v0.4.0 executable. RSS is the scanner child process,
measured by a fresh POSIX resource worker; it excludes Vitest and its dependencies.

| Project / pinned revision | Exercised source and behavior | Files / functions | Covered scan | Peak RSS |
| --- | --- | ---: | ---: | ---: |
| [Hono v4.6.12](https://github.com/honojs/hono/tree/76b7109d0c15dc85a947741593630460224f7b81) | `src`, real service route/JSON parameters and 404 through `app.request` | 166 / 956 | 1.013 s | 17.80 MiB |
| [TanStack Table v8.21.3](https://github.com/TanStack/table/tree/f4dc742b7b8bf01bb7dd10ee7d2f238400befcc0) | core + React workspace sources, `useReactTable` SSR with a real core row model | 38 / 710 | 0.218 s | 17.55 MiB |
| [Vite React TypeScript app](https://github.com/vitejs/vite/tree/e954a9cda0b5254be495a937333a0be1b8503d8f/packages/create-vite/template-react-ts) | actual generated TSX app, React hooks/assets/CSS through Vite and SSR | 2 / 3 | 0.070 s | 17.82 MiB |

Coverage collection plus the two covered scans took about 19.04, 9.81, and 8.01
seconds respectively. Every tracked upstream source remained unchanged; all three
coverage imports completed with zero mapping failures and identical repeated
scanner reports. The harness's test/config files are separate additions. It uses
locked tooling outside those repositories and an explicit core-workspace alias
for TanStack; it does not install/run each upstream project's full build system.

## Friction, scores, and supported limits

Hono exposed two parser defects: valid semicolon-free generic call signatures in
interface/object types and TypeScript 5 `export type *` declarations. The scanner
now normalizes only those type-only parser-error patterns, reparses the entire
source, and still rejects remaining errors. Runtime positions and original-byte
coverage hashes are preserved. Regression tests compare identities and locations
and reject malformed type/runtime/JSX syntax. Recovery is bounded to 64 parsing
passes; other unsupported grammar syntax remains an explicit input error.

Default argument declarations also exposed missing Istanbul statement evidence.
The importer now consumes direct JSON `default-arg` counters with their own
locations, including zero counts. LCOV lacks those records and can still miss the
declaration line. Unknown/invalid source mappings are rejected, never inferred
from arbitrary branch totals. Generated-JavaScript mapping limitations remain in
the [framework guide](FRAMEWORK_COVERAGE.md).

These smoke tests intentionally leave many paths unexecuted. Hono retained 39
files / 63 functions with missing evidence, 899 uncovered functions, and 28
partially covered functions. TanStack retained 2 files / 24 functions with missing
evidence, 589 uncovered functions, and 23 partially covered functions. Enabling
`require_coverage` correctly blocks those incomplete selections; no threshold or
completeness rule was relaxed to obtain a passing integration test. Their
absolute score gates returned 1, which the harness records as valid results.

Hono's unexecuted JSX `build` function has CC 59 and CRAP 3540; TanStack's unexecuted
filter/sort callbacks reach CC 17 and CRAP 306. These are expected `CC² + CC`
scores at zero coverage, not upstream quality judgments based on full test runs.
The Vite app passes threshold 10 even though its two uninvoked click callbacks
have zero hits: CC 1 gives CRAP 2. A score gate alone cannot promise every handler
was tested. Its SSR-rendered parent has full line evidence. Use project tests and
coverage policy appropriate to the behavior you want to enforce.

The local Android Node loader could not load native Vitest bindings under the
proot `/root` path. Moving only the ignored tooling dependency directory to
Termux's physical temporary directory resolved it; sources and dependency
versions were unchanged. Hosted Linux/macOS/Windows framework CI uses ordinary
native paths. Android has no prebuilt ts-crap release support.

## Resource-budget decision for v1

Configurable hard resource budgets and child-command timeouts do **not** block
v1 for the current trusted local CLI. The largest measured subset contains 956
functions across 166 files and scans in about one second with under 18 MiB peak
scanner RSS on this machine. Existing deep/nested/pathological traversal tests
also exercise iterative analysis. These results support the declared local-use
scope; they do not establish bounds for arbitrary repositories or a hosted service.

The CLI/helpers do not promise a sandbox, hard CPU/memory limit, or child timeout.
The harness has bounded subprocess timeouts and CI jobs have explicit deadlines;
those are validation safeguards, not new product guarantees. Users running
untrusted repositories must provide process/container isolation and resource
limits. A hosted or untrusted-command mode would require a separate budget and
cancellation design before release. Revisit the decision when larger-project
measurements or adoption failures establish a concrete need.
