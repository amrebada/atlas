# atlas-relay-stub

Tiny single-binary WebSocket relay for testing the `atlas-agent` (Phase 4
of the remote-control feature) before the production relay exists.

## What it does

- Accepts WebSocket connections at `/agent` (Bearer auth, any non-empty token)
- Logs every inbound message from agents (`<- agent: …`)
- Broadcasts every line typed on stdin to all connected agents (`-> agent: …`)

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

The relay logs the agent's hello envelope on connect.

## End-to-end test (Phase 4.0b)

The agent now forwards `rpc` envelopes to the local MCP server. **Make
sure MCP is enabled in Settings → Advanced first**, otherwise the agent
will reply with an error.

In the relay's stdin, paste a one-line JSON envelope:

```json
{"type":"rpc","id":"req-1","request":{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}}
```

Hit Enter. You should see:

```
-> agent: {"type":"rpc","id":"req-1","request":…}              # broadcast
<- agent: {"type":"rpc","id":"req-1","response":{"jsonrpc":"2.0","id":1,"result":{"tools":[…]}}}
```

The `id` round-trips so a real mobile client can correlate request →
response. The MCP layer's normal auth + bound-step approval all run
unchanged on the agent's local-loopback hop, so any `tools/call` that
mutates state will still pop the approval modal in Atlas.

### Try a tool call

```json
{"type":"rpc","id":"req-2","request":{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"atlas_list_recents","arguments":{"limit":3}}}}
```

You'll see the recents JSON come back over the wire.

### Mutating tools require approval

```json
{"type":"rpc","id":"req-3","request":{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"approval_request_plan","arguments":{"summary":"Pin atlas","steps":[{"tool":"atlas_pin_project","args":{"id":"<some-id>","pinned":true}}]}}}}
```

The Atlas modal pops; click Approve; the response (with `scopedToken`)
comes back through the relay. Then send the actual `atlas_pin_project`
call with that token and `step_index: 0` and watch Atlas's sidebar
update live.

## Not yet

This is a stub. The production relay (Phase 7) will:

- Verify per-device auth (ed25519 + nonce, not bearer)
- Route messages by device id between mobile and desktop
- Buffer for offline desktops
- Handle push notifications (APNs / FCM)
- Provide STT proxy for voice input
