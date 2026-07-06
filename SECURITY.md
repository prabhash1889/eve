# Security Policy

## Reporting

Please report security issues privately to the repository owner instead of opening
a public issue. Include the affected version or commit, reproduction steps, and
the expected impact.

## Secret Handling

Do not commit:

- Groq API keys or other service credentials
- Tauri updater private signing keys
- Authenticode, Azure, Apple, or other release-signing credentials
- `.env*` files, local model downloads, release artifacts, or build output

The Tauri updater public key in `src-tauri/tauri.conf.json` is intended to be
public. The matching private key must live only in a local secret store or GitHub
Actions secret.

Eve stores user API keys through the operating system credential store. Project
settings should never contain raw API keys.
