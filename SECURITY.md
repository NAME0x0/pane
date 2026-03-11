# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in Pane, **do not open a public issue**.

Instead, please report it privately by emailing the maintainers or using GitHub's [private vulnerability reporting](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability).

We take security seriously. Pane interacts with system-level components (WSL2, XRDP, display protocols), so any vulnerability in the pipeline matters.

## Scope

Security issues we care about:

- Privilege escalation through the launcher or daemon
- Credential exposure in configuration or logs
- Unsafe IPC between Windows and WSL2 components
- Injection vulnerabilities in command execution
- Insecure default configurations

## Response

We will acknowledge reports within 48 hours and provide a fix or mitigation plan within 7 days for confirmed vulnerabilities.
