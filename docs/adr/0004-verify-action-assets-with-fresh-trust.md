# Verify Action assets with a fresh Sigstore trust root

Each setup Action install fetches the current Sigstore trusted root and checks
the archive against the attested digest for its exact asset name and release
workflow identity. Fetching the root alongside the archive reduces the wait
while preserving trust freshness. Restoring the root from a cache or vending it
with the Action would make that stored copy the trust anchor and delay trust
updates. Per-asset checksum sidecars remain useful for manual downloads.

See [Action verification](../CI_ADOPTION.md#github-action) and the
[install measurements](../measurements/install-action-2026-11-14.json).
