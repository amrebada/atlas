// Atlas relay stub — Phase 4 prototype.
//
// Tiny single-binary WebSocket relay for testing the atlas-agent before
// the production relay (auth, push, multi-device routing, offline buffer)
// exists. Accepts an outbound WS from atlas-agent at ws://<host>/agent,
// dumps every inbound message to stdout, and broadcasts every line typed
// on stdin to all connected agents.
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
//
// You'll see the agent's "hello" arrive in this stub's logs. Type a JSON
// line and it gets pushed to every connected agent (Atlas).
package main

import (
	"bufio"
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

var (
	connections   = make(map[*websocket.Conn]struct{})
	connectionsMu sync.Mutex
)

func registerConn(c *websocket.Conn) {
	connectionsMu.Lock()
	defer connectionsMu.Unlock()
	connections[c] = struct{}{}
}

func unregisterConn(c *websocket.Conn) {
	connectionsMu.Lock()
	defer connectionsMu.Unlock()
	delete(connections, c)
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
		log.Printf("<- agent: %s\n", msg)
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
