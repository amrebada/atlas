// Atlas relay stub — Phase 4 prototype.
//
// Tiny single-binary WebSocket relay for testing the atlas-agent before
// the production relay (auth, push, multi-device routing, offline buffer)
// exists. Accepts an outbound WS from atlas-agent at ws://<host>/agent,
// dumps every inbound message to stdout, and broadcasts every line typed
// on stdin to all connected agents.
//
// Phase 4.1: extracts the agent's ed25519 public key from the hello
// envelope and verifies the signature on every subsequent inbound
// message. Logs `[ok ✓]` or `[BAD SIG ✗]` next to each one so you can
// see Phase 4.1 working end-to-end.
//
// Usage:
//
//	cd open-source/relay
//	go run .                                  # listens on :9000
//	ATLAS_RELAY_ADDR=:9999 go run .           # custom port
//
// Then in another terminal start Atlas with:
//
//	ATLAS_AGENT_ENABLED=1 \
//	ATLAS_AGENT_URL=ws://localhost:9000/agent \
//	ATLAS_AGENT_TOKEN=dev-secret \
//	pnpm tauri dev
package main

import (
	"bufio"
	"bytes"
	"crypto/ed25519"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"os"
	"sync"

	"github.com/gorilla/websocket"
)

var upgrader = websocket.Upgrader{
	CheckOrigin: func(r *http.Request) bool { return true },
}

// connState tracks per-connection identity learned from the hello
// envelope. Subsequent messages are verified against this pubkey.
type connState struct {
	pubkey   ed25519.PublicKey
	deviceID string
}

var (
	connections   = make(map[*websocket.Conn]*connState)
	connectionsMu sync.Mutex
)

func registerConn(c *websocket.Conn) {
	connectionsMu.Lock()
	defer connectionsMu.Unlock()
	connections[c] = &connState{}
}

func unregisterConn(c *websocket.Conn) {
	connectionsMu.Lock()
	defer connectionsMu.Unlock()
	delete(connections, c)
}

func setConnIdentity(c *websocket.Conn, s *connState) {
	connectionsMu.Lock()
	defer connectionsMu.Unlock()
	connections[c] = s
}

// canonicalJSON produces the bytes the agent signed: alphabetical keys,
// no whitespace, no HTML-escaping (matches serde_json::to_vec on the
// agent side for ASCII envelopes — which all our envelope fields are).
func canonicalJSON(v interface{}) ([]byte, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		return nil, err
	}
	// json.Encoder appends a newline; strip it.
	return bytes.TrimRight(buf.Bytes(), "\n"), nil
}

// verifyEnvelope re-serializes the payload without `sig` and verifies
// the ed25519 signature against `pubkey`. Returns false for any failure
// mode (bad shape, missing fields, non-32-byte key, non-64-byte sig,
// canonical mismatch).
func verifyEnvelope(raw []byte, pubkey ed25519.PublicKey) bool {
	var env map[string]interface{}
	if err := json.Unmarshal(raw, &env); err != nil {
		return false
	}
	sigHex, ok := env["sig"].(string)
	if !ok {
		return false
	}
	delete(env, "sig")

	canonical, err := canonicalJSON(env)
	if err != nil {
		return false
	}
	sig, err := hex.DecodeString(sigHex)
	if err != nil || len(sig) != ed25519.SignatureSize {
		return false
	}
	return ed25519.Verify(pubkey, canonical, sig)
}

// extractIdentity pulls device_id + public_key out of a hello envelope.
func extractIdentity(raw []byte) (*connState, error) {
	var env map[string]interface{}
	if err := json.Unmarshal(raw, &env); err != nil {
		return nil, err
	}
	if env["type"] != "hello" {
		return nil, fmt.Errorf("expected hello, got %v", env["type"])
	}
	pubHex, ok := env["public_key"].(string)
	if !ok {
		return nil, fmt.Errorf("missing public_key")
	}
	pub, err := hex.DecodeString(pubHex)
	if err != nil {
		return nil, fmt.Errorf("bad public_key hex: %w", err)
	}
	if len(pub) != ed25519.PublicKeySize {
		return nil, fmt.Errorf("public_key wrong size: %d (want 32)", len(pub))
	}
	deviceID, _ := env["device_id"].(string)
	return &connState{pubkey: ed25519.PublicKey(pub), deviceID: deviceID}, nil
}

func handler(w http.ResponseWriter, r *http.Request) {
	auth := r.Header.Get("Authorization")
	if len(auth) < 8 || auth[:7] != "Bearer " {
		http.Error(w, "missing bearer", http.StatusUnauthorized)
		return
	}
	token := auth[7:]

	conn, err := upgrader.Upgrade(w, r, nil)
	if err != nil {
		log.Println("upgrade:", err)
		return
	}
	defer conn.Close()

	registerConn(conn)
	defer unregisterConn(conn)
	log.Printf("connected token=***%s\n", tail(token, 4))

	for {
		_, msg, err := conn.ReadMessage()
		if err != nil {
			break
		}

		// First message must be a hello — extract identity + verify
		// against itself. Subsequent messages verify against the
		// pubkey we learned at hello.
		connectionsMu.Lock()
		state := connections[conn]
		connectionsMu.Unlock()

		var label string
		if state == nil || state.pubkey == nil {
			id, idErr := extractIdentity(msg)
			if idErr != nil {
				label = fmt.Sprintf("[hello expected: %v]", idErr)
			} else if !verifyEnvelope(msg, id.pubkey) {
				label = "[BAD HELLO SIG ✗]"
			} else {
				setConnIdentity(conn, id)
				label = fmt.Sprintf("[ok ✓ device=%s]", id.deviceID)
			}
		} else if verifyEnvelope(msg, state.pubkey) {
			label = "[ok ✓]"
		} else {
			label = "[BAD SIG ✗]"
		}

		log.Printf("<- agent %s %s\n", label, msg)
	}
	log.Println("disconnected")
}

// broadcastFromStdin reads JSON lines from stdin and pushes each to every
// connected agent. Useful for hand-driving the agent during testing.
func broadcastFromStdin() {
	scanner := bufio.NewScanner(os.Stdin)
	for scanner.Scan() {
		text := scanner.Text()
		if text == "" {
			continue
		}
		connectionsMu.Lock()
		for conn := range connections {
			if err := conn.WriteMessage(websocket.TextMessage, []byte(text)); err != nil {
				log.Println("write:", err)
				continue
			}
			log.Printf("-> agent: %s\n", text)
		}
		connectionsMu.Unlock()
	}
}

func tail(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[len(s)-n:]
}

func main() {
	addr := ":9000"
	if v := os.Getenv("ATLAS_RELAY_ADDR"); v != "" {
		addr = v
	}
	http.HandleFunc("/agent", handler)
	go broadcastFromStdin()
	log.Printf("atlas-relay-stub listening on %s/agent\n", addr)
	fmt.Println("Type a line and press Enter to broadcast to all connected agents.")
	log.Fatal(http.ListenAndServe(addr, nil))
}
