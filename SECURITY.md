# Security and privacy

Mélimo is an unofficial client. The first release is still being validated.

Never put session cookies, signed media URLs, account responses or private listening
information in public issues. If GitHub private vulnerability reporting is enabled,
use the repository's Security tab; otherwise open a minimal issue asking for a
private contact without disclosing an exploit or any credentials.

## Credential handling

ARL values are validated and debug-redacted. Login files are plaintext, not keychain
entries. Unix storage requires an owner-only directory and regular file; opening
uses no-follow/nonblocking flags, ownership and hard-link checks, and bounded reads.
Unsafe existing directories are rejected rather than silently chmodded. Temporary
files use exclusive creation with unpredictable names and are atomically renamed.
Storage is disabled on platforms without these Unix protections.

Only authenticated interactive logins are saved. Environment credentials stay in
memory. `MELIMO_NO_STORE=1` disables reads/writes; `--forget` removes a saved file.
There is no promise of memory zeroization, secure erase, protection from same-user
malware, privileged processes, OS swap, crash dumps or backups.

## Network and terminal boundaries

Production endpoints require HTTPS and reject redirects. Cookies are scoped to the
fixed gateway; media requests do not carry them. Responses and transfer sizes are
limited, stalled connections time out, and raw network errors are not displayed.
Provider metadata is stripped of terminal controls; terminal titles also receive
a final control-character filter and length bound.

## Before public publication

Audit both the current files and **all reachable Git history**, including commit
author/committer emails. Removing text in a new commit does not remove it from old
commits. Re-scan after the final squash and inspect branches, tags, PRs, workflow
logs and uploaded assets. A clean working tree or an automated pattern scan alone
cannot establish that a repository contains no personal information.

See `docs/RELEASE.md` for the remaining publication gates.
