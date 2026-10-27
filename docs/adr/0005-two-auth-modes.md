# Two auth modes: built-in login by default, trusted header opt-in

The app ships its own login (argon2id passwords, server-side sessions) and uses it by default, so any household can run it with nothing in front but an HTTPS reverse proxy. A second mode, chosen by env, trusts a username header (default `Remote-User`) from a configured proxy address, for deployments that already have single sign-on in front of their apps. Exactly one mode is active per install. Both modes share the same logins table, setup code and invites, so switching modes later needs no new code, only mapping each login to its header username.

## Considered Options

- **Header only (no passwords in the app)**: simplest for a home lab with SSO, but every other household would have to set up an SSO proxy first.
- **Built-in only**: blocks deployments that want one sign-on for all their apps.

## Consequences

- Header mode is only as safe as the proxy: the app rejects the header from any other source address, and the deployment must keep the app port reachable only from the proxy.
- No email anywhere: password recovery is a reset link issued by another login or by a CLI command on the server.
