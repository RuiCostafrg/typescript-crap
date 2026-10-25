# P2 correctness and report streaming

The October 24 security/Rust/performance review's unconditional P2 items 3–8
are implemented in v0.4.0. The original v1 coverage and baseline schemas remain
unchanged, and existing baselines do not need regeneration.

- Source identities are built from platform path components, checked before
  selection-rule matching, and checked for uniqueness among retained files.
  Literal backslashes/colons and non-UTF-8 paths cannot be represented by v1 and
  fail explicitly. Both Node snapshot collectors reject backslashes/colons too.
- Fingerprint fallback requires compatible qualified names. Anonymous location
  labels are normalized without including their line/column in matching; named
  containing scopes remain significant. Identity ambiguity remains conservative.
- Every baseline application starts from `crap > threshold`; comparisons do not
  compose, and completeness failure remains independent.
- XML has its own attribute encoder using the XML 1.0 scalar ranges. Forbidden
  scalars become U+FFFD; legal tab/newline/carriage-return attributes use numeric
  references. HTML retains its existing encoder.
- An iterative JSON token pass rejects repeated decoded keys at every object
  depth before `JSON.parse` creates Istanbul or source-map objects. `JSON.parse`
  still owns complete grammar validation and materialization. This adds no
  dependency and no recursive parser call stack. Tests cover escaped equivalents,
  sibling scopes, malformed grammar, generated values, and deep arrays.
- SARIF uses borrowed typed structures and a sequence serializer that creates
  one finding at a time. It preserves rule IDs, caller order, fingerprints,
  UTF-16 regions, percent-encoded URIs and completeness findings.
- Public writer APIs back the compatible string-returning APIs. CLI file output
  streams through buffered owned temporary files, flushing and syncing all
  staging files before publication. Final destination checks and individual
  atomic replacements remain. A group of replacements is not a transaction.
  Stdout is buffered before baseline publication; direct library writer calls
  can leave partial bytes on writer failure, as documented.

Focused tests exercise inferred arrows, assigned expressions, object properties,
class fields, moved functions, shifted anonymous callbacks, repeated baseline
comparisons, XML parsed with Python ElementTree, all-format writer failures,
SARIF semantics and staging cleanup with preservation of both old outputs.
Existing actual Node/V8, source-map, Git, alias and publication tests are retained.

Conditional item 9 (input, parser and subprocess budgets) remains deferred under
the review's stated local trusted-command CLI scope. Hosted/untrusted scanning
is not introduced by this release. Defaults need representative-project data;
parser cancellation alone would not bound I/O, analysis or child process trees.
This release makes no sandbox or bounded-resource guarantee.

## Local verification and measurements

The complete debug suite passed (131 Rust tests before one additional anonymous
label unit test); the final focused library/adoption suites also passed. Strict
all-target Clippy, formatting, rustdoc, 12 Python helper tests and all 32 Node
unit tests passed. Hosted CI remains authoritative for Rust 1.90, macOS/Windows,
release-profile tests, advisory scanning, 95% production coverage and 90% mutation
floors; these gates are unchanged.

The optimized `review` benchmark ran in fresh processes at 2,000 and 10,000
functions, with one warmup and five samples. Raw timings, output sizes, toolchain
and peak RSS are in `measurements/p2-rendering-2026-10-25.json`. At 10,000
functions SARIF's string wrapper took a median 20.033 ms with 59,372 KiB process
peak RSS; direct serialization to `io::sink` took 6.660 ms with 49,584 KiB.
At 2,000 functions those values were 3.544 ms / 18,128 KiB and 1.337 ms /
14,556 KiB. Output byte counts agree between string and streaming modes.

These are current-implementation measurements, not a controlled before/after
comparison. Sink output omits file I/O and can allow compiler optimizations;
process peak RSS includes source generation and parsing. Background compilation
and device scheduling can affect timing. The structural removal of SARIF JSON
value trees and file-output strings is established by the implementation; no
universal throughput or memory reduction percentage is claimed.
