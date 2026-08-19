package p

import "net/http"

func f() {
	resp, err := http.Get("http://example.com")
	defer resp.Body.Close()
	_ = err
}

// A method on *http.Client, and on an addressable http.Client value: upstream
// accepts either as the receiver of the call.
func clientPtr(c *http.Client, req *http.Request) {
	resp, err := c.Do(req)
	defer resp.Body.Close()
	_ = err
}

func clientValue(c http.Client, req *http.Request) {
	resp, err := c.Do(req)
	defer resp.Body.Close()
	_ = err
}

// Upstream walks every call in the file, so a block nested inside a loop or a
// func literal is not a hiding place.
func inLoop(urls []string) {
	for _, u := range urls {
		resp, err := http.Get(u)
		defer resp.Body.Close()
		_ = err
	}
}

func inFuncLit() func() {
	return func() {
		resp, err := http.Get("http://example.com")
		defer resp.Body.Close()
		_ = err
	}
}

type holder struct{ resp *http.Response }

// The reported name is the *root* of the assigned selector, not the response:
// `rootIdent` walks `h.resp` down to `h`, and the defer's `h.resp.Body.Close`
// down to the same `h`.
func (h *holder) field() {
	var err error
	h.resp, err = http.Get("http://example.com")
	defer h.resp.Body.Close()
	_ = err
}
