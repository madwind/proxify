package main

import (
	"log"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"proxify/config"
	"proxify/handler"

	"golang.org/x/net/http2"
	"golang.org/x/net/http2/h2c"
)

func main() {
	socketPath := config.AppConfig.SocketPath

	if err := os.MkdirAll(filepath.Dir(socketPath), 0755); err != nil {
		log.Fatalf("Failed to create socket directory: %v", err)
	}

	if _, err := os.Stat(socketPath); err == nil {
		if err := os.Remove(socketPath); err != nil {
			log.Fatalf("Failed to remove existing socket: %v", err)
		}
	}

	listener, err := net.Listen("unix", socketPath)
	if err != nil {
		log.Fatalf("Failed to listen on socket: %v", err)
	}
	defer func(listener net.Listener) {
		err := listener.Close()
		if err != nil {
			log.Fatalf("Failed to close socket: %v", err)
		}
	}(listener)

	if err := os.Chmod(socketPath, 0666); err != nil {
		log.Fatalf("Failed to chmod socket: %v", err)
	}

	http.HandleFunc(config.AppConfig.ProxyPath, handler.AuthMiddleware(handler.ProxyHandler))

	server := &http.Server{
		Handler: h2c.NewHandler(http.DefaultServeMux, &http2.Server{}),
	}

	log.Printf("Proxify %s listening on socket %s, proxy path: %s\n", Version, socketPath, config.AppConfig.ProxyPath)
	log.Fatal(server.Serve(listener))
}
