# Run scans and coverage collection as trusted local tools

The scanner and coverage helpers target local checkouts and trusted CI test
commands. Keep sandboxing and process budgets with the caller. The adoption
checks supported this scope, while hosting arbitrary repositories would require
a design for isolation, input limits, and cancellation.

See the [compatibility contract](../COMPATIBILITY.md#rust-api-and-dependencies)
and [adoption observations](../measurements/adoption-v1.json).
