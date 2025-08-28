package main

import (
	"log"
	"net/http"
	"proxify/config"
	"proxify/handler"
)

func main() {
	cfg := config.LoadConfig()
	http.HandleFunc(cfg.ProxyPath, handler.ProxyHandler)
	log.Printf("Proxify listening on :8080, proxy path: %s\n", cfg.ProxyPath)
	log.Fatal(http.ListenAndServe(":8080", nil))
}
