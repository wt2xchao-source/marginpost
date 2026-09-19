# Security Policy

## Supported Version

MarginPost is currently pre-release. Security fixes are applied to the latest
code on the default branch; no older version receives separate support.

## Reporting a Vulnerability

Do not disclose a vulnerability in a public issue, discussion, or pull request.

Use GitHub private vulnerability reporting for the MarginPost repository once
the public repository is available. If private reporting is unavailable, open
a public issue requesting a private contact channel without including exploit
details, affected file paths, logs, or user data.

Include:

- affected version or commit;
- operating system;
- reproduction steps;
- expected and actual behavior;
- impact assessment;
- a minimal proof of concept with secrets and personal data removed.

Reports will be acknowledged after review. Disclosure timing will be coordinated
after a fix or mitigation is available.

## Security Boundaries

MarginPost is local-first and does not currently require accounts, API keys,
payment services, telemetry, or cloud storage. Local Markdown files and review
history remain under the user's operating-system account and filesystem
permissions.
