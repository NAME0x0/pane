# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in Pane, **do not open a public issue**.

Instead, please report it privately by emailing the maintainers or using GitHub''s [private vulnerability reporting](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability).

Pane interacts with system-level components such as WSL2, XRDP, generated bootstrap scripts, and the Windows RDP handoff, so security issues in the Phase 1 pipeline matter.

## Scope

Security issues we care about:

- Privilege escalation through the launcher or generated bootstrap flow
- Credential exposure in configuration or logs
- Injection vulnerabilities in command execution
- Insecure default configurations for XRDP or session startup
- Unsafe handling of persisted launch state or generated connection assets

## Response

We will acknowledge reports within 48 hours and provide a fix or mitigation plan within 7 days for confirmed vulnerabilities.