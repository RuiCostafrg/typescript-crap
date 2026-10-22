# Reviewed dependency updates

Dependabot opens weekly Cargo and GitHub Actions update PRs. There is no automatic
merge. The owner reviews changelogs, lockfile changes, MSRV compatibility, action
permissions and immutable SHA pins, then applies the reviewed update in an
owner-authored PR. Bot-authored events intentionally do not run trusted PR jobs:
never loosen the numeric actor gate or use `pull_request_target` to execute them.
The owner PR must pass all checks, including audit, coverage and mutation gates.

`Scheduled dependency advisories` audits the current default branch daily and
supports owner manual dispatch. Its schedule condition explicitly allows scheduled
events regardless of the recorded scheduling actor, while the fixed repository
check remains. It has read-only permissions, no publishing secrets, and never
checks out pull-request code. This narrow schedule exception does not alter the
owner-only actor gate on PR/build/release workflows. Investigate failed scans and
update dependencies through the same reviewed process; do not ignore advisories
merely to obtain a green result.
