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
	log.Printf("Proxify listening on :%s, proxy path: %s\n", cfg.ProxyPort, cfg.ProxyPath)
	log.Fatal(http.ListenAndServe(":"+cfg.ProxyPort, nil))
}
