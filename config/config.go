package config

import (
	"os"
)

type Config struct {
	ProxyPath string
	ProxyPort string
}

func LoadConfig() *Config {
	proxyPath := os.Getenv("PROXY_PATH")
	if proxyPath == "" {
		proxyPath = "/proxy"
	}
	proxyPort := os.Getenv("PROXY_PORT")
	if proxyPort == "" {
		proxyPort = "80"
	}

	return &Config{
		ProxyPath: proxyPath,
		ProxyPort: proxyPort,
	}
}
