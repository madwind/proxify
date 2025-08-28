package config

import (
	"os"
)

type Config struct {
	ProxyPath string
}

func LoadConfig() *Config {
	proxyPath := os.Getenv("PROXY_PATH")
	if proxyPath == "" {
		proxyPath = "/proxy"
	}

	return &Config{
		ProxyPath: proxyPath,
	}
}
