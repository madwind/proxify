package handler

import (
	"crypto/tls"
	"io"
	"log"
	"net/http"
	"net/url"
	"proxify/config"
	"strings"
	"sync"
	"time"
)

var ignoreHeaders = []string{"host", "origin", "referer", "cdn-loop", "cf-", "x-", "range"}
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
	targetURL := r.URL.Query().Get("url")
	if targetURL == "" {
		http.Error(w, "Missing url", http.StatusBadRequest)
		return
	}
	var u *url.URL
	var err error

	upstream := r.URL.Query().Get("upstream")
	if upstream != "" {
		u, err = url.Parse("https://" + upstream + config.AppConfig.ProxyPath)
		if err != nil {
			http.Error(w, "Invalid upstream", http.StatusBadRequest)
			return
		}
		q := u.Query()
		q.Set("url", targetURL)
		u.RawQuery = q.Encode()
	} else {
		u, err = url.Parse(targetURL)
		if err != nil {
			http.Error(w, "Invalid url", http.StatusBadRequest)
			return
		}
	}

	reqHeaders := buildRequestHeader(r.Header, targetURL)

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
	w.WriteHeader(resp.StatusCode)
	buf := bufferPool.Get().([]byte)
	defer bufferPool.Put(buf)
	written, err := io.CopyBuffer(w, resp.Body, buf)
	if err != nil {
		log.Printf("Error copying response body for %s: %v", targetURL, err)
	}

	log.Printf("Proxy request -> %s , status=%d , size=%d bytes", targetURL, resp.StatusCode, written)

}

func buildRequestHeader(header http.Header, targetURL string) http.Header {
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
				skip = true
				break
			}
		}
		if !skip {
			newHeader[k] = vals
		}
	}
	newHeader.Set("Host", host)
	return newHeader
}
