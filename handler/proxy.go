package handler

import (
	"crypto/tls"
	"fmt"
	"io"
	"log"
	"net/http"
	"net/url"
	"proxify/config"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

var ignoreHeaders = []string{"host", "origin", "referer", "cdn-loop", "cf-", "x-", "range", "upgrade", "connection"}
var client = &http.Client{
	Timeout: 60 * time.Second,
	Transport: &http.Transport{
		TLSClientConfig:     &tls.Config{InsecureSkipVerify: true},
		DisableCompression:  false,
		MaxIdleConnsPerHost: 10,
	},
}
var bufferPool = sync.Pool{
	New: func() interface{} {
		return make([]byte, 128*1024) // 32KB 默认缓冲区
	},
}

func ProxyHandler(w http.ResponseWriter, r *http.Request) {
	start := time.Now()
	targetURL := r.URL.Query().Get("url")
	if targetURL == "" {
		http.Error(w, "Missing url", http.StatusBadRequest)
		return
	}
	var u *url.URL
	var err error

	upstream := r.URL.Query().Get("upstream")
	token := r.URL.Query().Get("token")
	if len(config.AppConfig.JwtKey) > 0 {
		if !validateToken(token) {
			http.Error(w, "Unauthorized: Invalid token", http.StatusUnauthorized)
			return
		}
	}
	upstreamUsed := upstream != ""
	if upstreamUsed {
		u, err = url.Parse("https://" + upstream + config.AppConfig.ProxyPath)
		if err != nil {
			http.Error(w, "Invalid upstream", http.StatusBadRequest)
			return
		}
		q := u.Query()
		q.Set("url", targetURL)
		q.Set("token", token)
		u.RawQuery = q.Encode()
	} else {
		u, err = url.Parse(targetURL)
		if err != nil {
			http.Error(w, "Invalid url", http.StatusBadRequest)
			return
		}
	}

	reqHeaders := buildRequestHeader(r.Header, targetURL, upstreamUsed)

	req, err := http.NewRequest(r.Method, u.String(), nil)
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	req.Header = reqHeaders

	resp, err := client.Do(req)
	if err != nil {
		http.Error(w, err.Error(), http.StatusBadGateway)
		return
	}
	defer func(Body io.ReadCloser) {
		err := Body.Close()
		if err != nil {
		}
	}(resp.Body)

	for k, vals := range resp.Header {
		for _, v := range vals {
			w.Header().Add(k, v)
		}
	}

	contentLength := resp.Header.Get("Content-Length")
	var written int64
	var mode string

	if contentLength != "" {
		mode = "stream"
		w.WriteHeader(resp.StatusCode)
		buf := bufferPool.Get().([]byte)
		defer bufferPool.Put(buf)

		written, err = io.CopyBuffer(w, resp.Body, buf)
		if err != nil {
			log.Printf("Error copying response body for %s: %v", targetURL, err)
		}
	} else {
		mode = "read-all"
		body, err := io.ReadAll(resp.Body)
		if err != nil {
			http.Error(w, "Failed to read upstream body", http.StatusBadGateway)
			return
		}

		w.Header().Set("Content-Length", strconv.Itoa(len(body)))
		w.WriteHeader(resp.StatusCode)
		n, err := w.Write(body)
		written = int64(n)
		if err != nil {
			log.Printf("Error writing response body for %s: %v", targetURL, err)
		}
	}

	duration := time.Since(start)

	up := upstream
	if up == "" {
		up = "direct"
	}

	log.Printf(
		"Proxy request -> %s , upstream=%s , mode=%s , status=%d , size=%d bytes , cost=%s",
		targetURL, up, mode, resp.StatusCode, written, duration,
	)
}

func buildRequestHeader(header http.Header, targetURL string, preserveRange bool) http.Header {
	newHeader := http.Header{}
	u, err := url.Parse(targetURL)
	if err != nil {
		return newHeader
	}
	host := u.Host
	for k, vals := range header {
		lower := strings.ToLower(k)

		skip := false
		for _, ign := range ignoreHeaders {
			if strings.HasPrefix(lower, ign) {
				if preserveRange && ign == "range" {
					skip = false
				} else {
					skip = true
				}
			}
		}
		if !skip {
			newHeader[k] = vals
		}
	}
	newHeader.Set("Host", host)
	return newHeader
}
func validateToken(tokenString string) bool {
	if tokenString == "" {
		return false
	}
	token, err := jwt.Parse(tokenString, func(token *jwt.Token) (interface{}, error) {
		if _, ok := token.Method.(*jwt.SigningMethodHMAC); !ok {
			return nil, fmt.Errorf("unexpected signing method: %v", token.Header["alg"])
		}
		return config.AppConfig.JwtKey, nil
	})
	return err == nil && token.Valid
}
