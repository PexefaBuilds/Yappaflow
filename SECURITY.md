# Security policy

## Reporting a vulnerability

Please report privately, not in a public issue:

- **Preferred:** GitHub **Private vulnerability reporting**: the repository's **Security** tab → **Report a vulnerability**.

We acknowledge reports within **7 days** and keep you updated until it's fixed. We're happy to credit you in the release notes.

**Never send us an API key**, yours or anyone else's, and never include one in logs or screenshots. If a key may have been exposed, revoke it at the provider first.

## Scope

In scope:

- **Key handling:** API keys leaving the OS keychain, appearing in logs, files, the database, crash output, or being sent anywhere other than that key's provider.
- **Updater and release signing:** anything that lets an unsigned or tampered update or installer be accepted.
- **Prompt injection that types unintended text:** dictated or on-screen content (window titles, dictionary terms, the transcript) that makes the cleanup model insert text the user didn't say, run instructions, or leak other context.
- **Data going somewhere [PRIVACY.md](PRIVACY.md) doesn't list.**
- Local privilege issues in the keyboard hook, text insertion or clipboard handling.

Out of scope: vulnerabilities in a provider's own service (report those to the provider), and attacks that need an already-compromised user account.

## Supported versions

Only the latest release gets security fixes.
