package handler

import (
	"crypto/tls"
	"io"
	"log"
	"net/http"
	"net/url"
	"proxify/service"
	"strconv"
	"strings"
	"time"
)

var ignoreHeaders = []string{"host", "origin", "referer", "cdn-loop", "cf-", "x-", "range"}

func ProxyHandler(w http.ResponseWriter, r *http.Request) {
	targetURL := r.URL.Query().Get("url")
	if targetURL == "" {
		http.Error(w, "Missing url", http.StatusBadRequest)
		return
	}
	upstream := r.URL.Query().Get("upstream")
	if upstream != "" {
		targetURL = upstream + url.QueryEscape(targetURL)
	}

	tsInfo := strings.EqualFold(r.URL.Query().Get("tsInfo"), "true")
	reqHeaders := buildRequestHeader(r.Header, targetURL)

	req, err := http.NewRequest(r.Method, targetURL, nil)
	if err != nil {
		http.Error(w, err.Error(), http.StatusInternalServerError)
		return
	}
	req.Header = reqHeaders

	client := &http.Client{
		Timeout: 60 * time.Second,
		Transport: &http.Transport{
			TLSClientConfig:     &tls.Config{InsecureSkipVerify: true},
			DisableCompression:  false,
			MaxIdleConnsPerHost: 10,
		},
	}

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

	if tsInfo {
		startTime, err := service.GetStartTimeFromStream(resp.Body)
		if err != nil {
			http.Error(w, err.Error(), http.StatusInternalServerError)
			return
		}

		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		_, err = w.Write([]byte(`{"url":"` + targetURL + `","startTime":` + strconv.FormatFloat(startTime, 'f', 6, 64) + `}`))
		if err != nil {
			return
		}
		return
	}

	for k, vals := range resp.Header {
		for _, v := range vals {
			w.Header().Add(k, v)
		}
	}
	w.WriteHeader(resp.StatusCode)

	written, err := io.Copy(w, resp.Body)
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
