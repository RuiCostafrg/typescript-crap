# Default to static musl for portable Linux installs

The Linux shell installer, npm platform packages, setup Action, and Debian
packages use the static musl build. Keeping a single default binary per
architecture avoids choosing based on a detected glibc version and gives Alpine
and older glibc distributions the same tested executable.

GNU archives remain available for direct download and serve Linux Homebrew
users. The static build uses mimalloc because threaded parsing allocates through
libc, and the hosted comparison showed allocator contention in musl. This
trades a C dependency and higher peak RSS in that comparison for faster scans.
See [Linux distribution](../LINUX_DISTRIBUTION.md) and the
[hosted observations](../measurements/vscode-hosted-after-wp5-2026-11-16.json).
