package main

import (
	"fmt"
	"log"
	"net/http"
	"proxify/config"
	"proxify/handler"
)

func main() {
	fmt.Printf("Proxify version: %s", Version)
	cfg := config.LoadConfig()
	http.HandleFunc(cfg.ProxyPath, handler.ProxyHandler)
	log.Printf("Proxify listening on :8080, proxy path: %s\n", cfg.ProxyPath)
	log.Fatal(http.ListenAndServe(":8080", nil))
}
