package p

import (
	"io"
	"net/http"
)

func Bad() error {
	resp, err := http.Get("http://example.com")
	if err != nil {
		return err
	}
	_ = resp
	return nil
}

// Handing the body to the caller is bodyclose's idea of handing over the
// close: `isCloseCall` answers yes for a `Return` whose results include an
// `io.ReadCloser`, and `resp.Body` is one.
func ReturnsBody() (io.ReadCloser, error) {
	resp, err := http.Get("http://example.com")
	if err != nil {
		return nil, err
	}
	return resp.Body, nil
}

// Wrapping the body is not returning it: the `Return` is no longer a referrer
// of the body load, so this one is still a finding.
func ReturnsWrappedBody() (io.ReadCloser, error) {
	resp, err := http.Get("http://example.com")
	if err != nil {
		return nil, err
	}
	return io.NopCloser(resp.Body), nil
}
