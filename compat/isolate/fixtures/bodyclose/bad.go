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

// Upstream tracks responses that come out of a *call*: a response reached any
// other way is not one this package opened, and none of these is a finding for
// either tool. dapr receives its responses over a channel
// (`tests/integration/suite/daprd/shutdown/graceful`).
func FromChannel(ch chan *http.Response) int {
	select {
	case resp := <-ch:
		return resp.StatusCode
	}
}

func FromMap(m map[string]*http.Response) int {
	resp := m["k"]
	return resp.StatusCode
}

func FromSlice(rs []*http.Response) int {
	resp := rs[0]
	return resp.StatusCode
}

func FromCopy(in *http.Response) int {
	resp := in
	return resp.StatusCode
}

// A response assigned with `=` rather than `:=` is still a response, and the
// report lands on the call's `(`.
func Reassigned(client *http.Client, req *http.Request) error {
	var resp *http.Response
	var err error
	resp, err = client.Do(req)
	if err != nil {
		return err
	}
	_ = resp.StatusCode
	return nil
}
