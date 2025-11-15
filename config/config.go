package config

import "os"

type Config struct {
	ProxyPath  string
	SocketPath string
}

var AppConfig = &Config{
	ProxyPath: func() string {
		if v := os.Getenv("PROXY_PATH"); v != "" {
			return v
		}
		return "/proxy"
	}(),
	SocketPath: func() string {
		if v := os.Getenv("SOCKET_PATH"); v != "" {
			return v
		}
		return "/dev/shm/proxify/proxify.sock"
	}(),
}
