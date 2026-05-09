# atlas-relay-stub

Tiny single-binary WebSocket relay for testing the `atlas-agent` (Phase 4
of the remote-control feature) before the production relay exists.

## What it does

- Accepts one or more WebSocket connections at `/agent`
- Requires `Authorization: Bearer <anything>` (any non-empty token works
  in stub mode — auth happens for real in the production relay)
- Dumps every inbound message to stdout, prefixed with `<- agent`
- Broadcasts every line typed on stdin to all connected agents,
  prefixed with `-> agent`

## Run

```sh
cd open-source/relay
go run .                          # :9000
ATLAS_RELAY_ADDR=:9999 go run .   # custom port
```

In another terminal, start Atlas with the agent enabled:

```sh
ATLAS_AGENT_ENABLED=1 \
ATLAS_AGENT_URL=ws://localhost:9000/agent \
ATLAS_AGENT_TOKEN=dev-secret \
pnpm tauri dev
```

The relay should log the agent's `hello` envelope. Type a JSON line in the
relay's terminal and Atlas will emit `agent:message` Tauri events with
the payload (visible in the dev console).

## Not yet

This is a stub. The production relay will:

- Verify per-device auth (ed25519 + nonce, not bearer)
- Route messages by device id between mobile and desktop
- Buffer for offline desktops
- Handle push notifications (APNs / FCM)
- Provide STT proxy for voice input

That work happens in Phase 7.
