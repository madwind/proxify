package config

import "os"

type Config struct {
	ProxyPath  string
	SocketPath string
	JwtKey     []byte
}

var AppConfig = &Config{
	ProxyPath: func() string {
		if v := os.Getenv("PROXY_PATH"); v != "" {
			return v
		}
		return "/api/proxy/file"
	}(),
	SocketPath: func() string {
		if v := os.Getenv("SOCKET_PATH"); v != "" {
			return v
		}
		return "/dev/shm/proxify.sock"
	}(),
	JwtKey: func() []byte {
		v := os.Getenv("JWT_SIGNING_KEY")
		if v != "" {
			return []byte(v)
		}
		return []byte("")
	}(),
}
