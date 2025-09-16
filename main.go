package main

import (
	"log"
	"net/http"
	"proxify/config"
	"proxify/handler"

	"golang.org/x/net/http2"
	"golang.org/x/net/http2/h2c"
)

func main() {
	http.HandleFunc(config.AppConfig.ProxyPath, handler.ProxyHandler)
	server := &http.Server{
		Addr:    ":" + config.AppConfig.ProxyPort,
		Handler: h2c.NewHandler(http.DefaultServeMux, &http2.Server{}),
	}
	log.Printf("Proxify %s listening on :%s, proxy path: %s\n", Version, config.AppConfig.ProxyPort, config.AppConfig.ProxyPath)
	log.Fatal(server.ListenAndServe())
}
