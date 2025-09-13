package config

import "os"

type Config struct {
	ProxyPath string
	ProxyPort string
}

var AppConfig = &Config{
	ProxyPath: func() string {
		if v := os.Getenv("PROXY_PATH"); v != "" {
			return v
		}
		return "/proxy"
	}(),
	ProxyPort: func() string {
		if v := os.Getenv("PROXY_PORT"); v != "" {
			return v
		}
		return "80"
	}(),
}
