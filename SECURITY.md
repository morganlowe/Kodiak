# Security Policy

## Supported Versions

Only the latest release receives security patches.

| Version | Supported          |
| ------- | ------------------ |
| 1.0.0   | :white_check_mark: |
| < 1.0.0 | :x:                |

## Reporting a Vulnerability

If you discover a security vulnerability or bypass in Kodiak, please **do not open a public issue**.

Report vulnerabilities privately via [GitHub Security Advisories](https://github.com/morganlowe/Kodiak/security/advisories/new).

### High-Priority Surfaces

We prioritize vulnerabilities affecting:
- **Granular Grant Matrix**: Escalation beyond `READ`, `CREATE`, or `BUILD` without human approval.
- **Hard Deny-List**: Any path traversal, symlink, or pattern bypass accessing credentials (`~/.ssh`, `~/.aws`, `~/.gcp`), `.env` files, or `.git/` history.
- **Filesystem Boundaries**: Any read, write, or command execution outside explicitly whitelisted project paths.
- **Atomic Write Path**: Any race condition or partial file mutation bypassing human audit confirmation.

### Triage Expectations

- Submissions will be acknowledged and triaged promptly.
- Verified fixes will be published with a release tag and security advisory before details are made public.
