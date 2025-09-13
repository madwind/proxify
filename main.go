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
	http.HandleFunc(config.AppConfig.ProxyPath, handler.ProxyHandler)
	log.Printf("Proxify listening on :%s, proxy path: %s\n", config.AppConfig.ProxyPort, config.AppConfig.ProxyPath)
	log.Fatal(http.ListenAndServe(":"+config.AppConfig.ProxyPort, nil))
}
