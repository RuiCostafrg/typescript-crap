# Migrating to ts-crap 3.0

ts-crap 3.0 changes two public fields of `FunctionScore` to cut per-function
memory. Every serialized report, baseline, SARIF document, and human-readable
output keeps the same bytes.

## `FunctionScore::identity`

`identity` is now `FunctionIdentity`, a 32-byte SHA-256 digest, instead of a
64-character `String`.

- Old `String` value: `score.identity.to_string()`.
- Raw digest bytes: `score.identity.as_bytes()`.

`FunctionIdentity` implements `Display` and `Serialize` as 64 lowercase
hexadecimal characters, and `FromStr` accepts exactly 64 hexadecimal
characters in either case.

## `FunctionScore::path`

`path` is now `Arc<str>` instead of `String`. One `Arc` is shared by every
function in a file.

- Old `&str` value: `&*score.path`.
- Owned `String`: `score.path.to_string()`.

Cloning the field is cheap and keeps the file path shared.
