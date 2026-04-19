# OAuth

Orbit supports both public (PKCE-only) and confidential (client-secret) OAuth clients. Tokens are wrapped by the macOS Keychain under a per-plugin service name (`com.orbit.plugin.<id>`), never written to disk in plaintext.

## Declare a provider

```jsonc
"oauthProviders": [
  {
    "id": "github",
    "name": "GitHub",
    "authorizationUrl": "https://github.com/login/oauth/authorize",
    "tokenUrl": "https://github.com/login/oauth/access_token",
    "scopes": ["repo", "read:org"],
    "clientType": "confidential",       // or "public" for PKCE-only
    "redirectUri": "http://127.0.0.1:47821/oauth/callback"
  }
]
```

`redirectUri` **must** be exactly `http://127.0.0.1:47821/oauth/callback`. Orbit runs a short-lived loopback HTTP server on that port (RFC 8252 § 7.3) and routes the callback into the OAuth handler. This pattern works identically on macOS, Windows, and Linux with no OS-level scheme registration.

## Public vs confidential

- **Public (PKCE)** — best when the provider supports it. Orbit generates a verifier + challenge per flow; no secret needed.
- **Confidential** — for providers that require a client secret (classic GitHub OAuth Apps). The user pastes their own `client_id` and `client_secret` into the Plugin detail drawer's OAuth tab before clicking Connect. Both are stored in Keychain.

## Flow

1. User clicks Connect on the Plugin detail drawer's OAuth tab.
2. Orbit generates PKCE + state, parks the verifier in memory (TTL 10 min).
3. System browser opens the authorization URL.
4. Provider redirects to `http://127.0.0.1:47821/oauth/callback?state=...&code=...`.
5. Orbit's loopback listener receives the GET, parses state + code, and invokes the callback handler.
6. Orbit exchanges the code at `tokenUrl` (adding `client_secret` for confidential clients).
7. Access and refresh tokens land in Keychain.
8. `plugin:oauth:connected` event fires; UI flips to "Connected".

## Token delivery

On subprocess spawn, every OAuth provider's access token becomes an env var: `ORBIT_OAUTH_<PROVIDER_ID_UPPER>_ACCESS_TOKEN`. The `@orbit/plugin-sdk` exposes these via `oauth.<providerId>.accessToken`.

## Refresh

V1 does not auto-refresh. If your token expires mid-call, return a structured error; the plugin author's next UI action triggers a manual reconnect. V1.1 adds refresh-token rotation.
