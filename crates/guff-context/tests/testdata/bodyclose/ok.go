package bodyclose

import (
	"io"
	"net/http"
	"net/http/httptest"
)

func withClose() {
	resp, err := http.Get("https://example.com")
	if err != nil {
		return
	}
	defer resp.Body.Close()
	_ = resp.StatusCode
}

func withDeferFunc() {
	resp, err := http.Get("https://example.com")
	if err != nil {
		return
	}
	defer func() { _ = resp.Body.Close() }()
	_ = resp.StatusCode
}

func withConsumeAndClose() {
	resp, err := http.Get("https://example.com")
	if err != nil {
		return
	}
	defer resp.Body.Close()
	_, _ = io.ReadAll(resp.Body)
}

func returnsResponse() (*http.Response, error) {
	return http.Get("https://example.com")
}

func returnsResponseNamed() (*http.Response, error) {
	resp, err := http.Get("https://example.com")
	if err != nil {
		return nil, err
	}
	return resp, nil
}

func blankResponse() {
	_, err := http.Get("https://example.com")
	if err != nil {
		return
	}
}

func httptestResult() {
	rec := &httptest.ResponseRecorder{}
	resp := rec.Result()
	_ = resp.StatusCode
}
