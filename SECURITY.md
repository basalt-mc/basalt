# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in Basalt, please report it responsibly.

**Do NOT open a public issue.** Instead, use one of these channels:

- **GitHub Security Advisories:** [Report a vulnerability](https://github.com/basalt-mc/basalt/security/advisories/new)
- **Email:** security@basalt-mc.com

### What to include

- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if you have one)

### Response timeline

- **Acknowledgment:** within 48 hours
- **Initial assessment:** within 7 days
- **Fix or mitigation:** depends on severity, targeting under 30 days for critical issues

## Scope

### In scope

- Buffer overflows or panics from malformed network packets
- Memory safety issues in protocol parsing
- Denial of service via resource exhaustion (memory, CPU)
- Authentication bypass (when auth is implemented)
- Any issue that could crash the server or allow unauthorized access

### Out of scope

- Minecraft game exploits that exist in vanilla (e.g., X-ray, duplication glitches)
- Issues requiring physical access to the server machine
- Social engineering

## Supported Versions

Only the latest release is supported with security fixes. We do not backport fixes to older versions.

| Version  | Supported |
| -------- | --------- |
| latest   | Yes       |
| < latest | No        |

## Security Design

Basalt is designed with security in mind:

- **Zero `unsafe` blocks** — the entire codebase uses safe Rust
- **Fuzz testing** — 10 fuzz targets run nightly against all protocol decoders
- **Bounded allocations** — `Vec::with_capacity` is capped to input size to prevent OOM from malicious length fields
- **cargo-deny** — dependency audits run in CI against the RustSec advisory database
