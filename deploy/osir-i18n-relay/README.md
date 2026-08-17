# OSIR Codex i18n relay

This is an intentionally narrow HTTP CONNECT relay used by manager-owned
Codex launches. It permits only `ab.chatgpt.com:443`, keeps TLS end-to-end,
and rejects every other target. It is not a general proxy and must not be
advertised as one.

The manager's PAC rule routes only `ab.chatgpt.com`; all other hosts return
`DIRECT`. The PAC includes a direct fallback if the relay is unavailable.

The relay cannot see Statsig JSON or know whether `enable_i18n` was enabled;
that would require HTTPS interception and is deliberately out of scope.

The manager's loopback proxy connects to `wss://app.osirclaw.com/i18n-tunnel`.
Caddy terminates the normal HTTPS connection on port 443 and forwards this
single path to the private WebSocket service, which only opens
`ab.chatgpt.com:443`. Keep the tunnel service private and monitor connection
counts.
