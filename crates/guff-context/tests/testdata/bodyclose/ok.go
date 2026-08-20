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

func httptestResult() {
	rec := &httptest.ResponseRecorder{}
	resp := rec.Result()
	_ = resp.StatusCode
}

func withCleanup(t interface{ Cleanup(func()) }) {
	resp, err := http.Get("https://example.com")
	if err != nil {
		return
	}
	t.Cleanup(func() {
		_ = resp.Body.Close()
	})
	_ = resp.StatusCode
}

func passToHelper() error {
	resp, err := http.Get("https://example.com")
	if err != nil {
		return err
	}
	return handleResponse(resp)
}

func handleResponse(resp *http.Response) error {
	defer resp.Body.Close()
	_, err := io.ReadAll(resp.Body)
	return err
}

func syntheticResponse() {
	res := &http.Response{
		StatusCode: 200,
	}
	_ = res.StatusCode
}
