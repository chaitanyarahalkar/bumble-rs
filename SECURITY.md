# Security Policy

## Reporting a vulnerability

Please report suspected security vulnerabilities **privately** — do not open a
public issue for them.

- Preferred: use GitHub's private vulnerability reporting
  ("Report a vulnerability" under the repository's **Security** tab), or
- Email the maintainers at **[INSERT SECURITY CONTACT]**.

Please include enough detail to reproduce (affected crate, input, and observed
behavior). We will acknowledge your report, work with you on a fix, and
coordinate disclosure.

## Supported versions

This project is pre-1.0; only the latest `main` is supported. Security fixes
land on `main`.

## Scope and expectations

bumble-rs is an educational, incremental port of a Bluetooth stack. It is
**not** a hardened, production Bluetooth implementation:

- It parses untrusted wire input (HCI/L2CAP/ATT/SMP PDUs). Parsing-robustness
  issues (panics, out-of-bounds, integer overflow on malformed input) are
  in scope and welcome as reports.
- The cryptographic code (`bumble-crypto`) implements Bluetooth SMP primitives
  and is validated against published test vectors, but it has **not** had an
  independent cryptographic audit and provides no side-channel guarantees. Do
  not rely on it for production security.
