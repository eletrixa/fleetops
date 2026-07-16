# Security Policy

## Scope

fleetops runs locally and read-only over your own machine's Claude Code sessions. It reads no
credentials (no tokens, no API keys) and makes no network requests. See the "What it reads &
privacy" section of the README for the exact set of files it touches.

## Reporting a vulnerability

Please report suspected vulnerabilities **privately** — do not open a public issue for a security
problem.

Use GitHub's [private vulnerability reporting](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability)
("Report a vulnerability" under the repository's **Security** tab).

Include enough to reproduce: affected version/commit, environment (WSL distro, Rust version), and a
minimal set of steps. You will get an acknowledgement as soon as the maintainers can respond; this
project is passively maintained, so please allow time before any public disclosure.
