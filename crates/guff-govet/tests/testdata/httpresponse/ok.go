package p

import (
	"net/http"

	nethttp "net/http"
)

func f() {
	resp, err := http.Get("http://example.com")
	if err != nil {
		return
	}
	defer resp.Body.Close()
}

// `(*http.Response, error)` is the whole membership test, not "the callee lives
// in net/http". These two are the package's most common counterexamples, and
// the second is the standard request-body idiom.
func maxBytes(w http.ResponseWriter, r *http.Request) {
	r.Body = http.MaxBytesReader(w, r.Body, 1<<20)
	defer r.Body.Close()
}

func newRequest() {
	req, err := http.NewRequest("GET", "http://example.com", nil)
	defer req.Body.Close()
	_ = err
}

// Wrapped by another call: upstream counts the calls between the response call
// and its block and gives up above one, because the error may well have been
// checked by the wrapper.
func wrapped(u string) {
	resp, err := passthrough(http.Get(u))
	defer resp.Body.Close()
	_ = err
}

func passthrough(r *http.Response, err error) (*http.Response, error) { return r, err }

// A bare `f()` is not a selector, so it never reaches the signature test.
func bare() {
	resp, err := localGet()
	defer resp.Body.Close()
	_ = err
}

func localGet() (*http.Response, error) { return nil, nil }

type fake struct{}

func (fake) Do(*http.Request) (*http.Response, error) { return nil, nil }

// Right signature, wrong receiver: the method has to be on an http.Client.
func notAClient(req *http.Request) {
	var c fake
	resp, err := c.Do(req)
	defer resp.Body.Close()
	_ = err
}

// The receiver test for a package-qualified call is on the *identifier*, so
// net/http imported under any other name is not net/http for this check.
func aliased() {
	resp, err := nethttp.Get("http://example.com")
	defer resp.Body.Close()
	_ = err
}

// `restOfBlock` looks for the innermost *BlockStmt*. A case or comm clause body
// is a bare []Stmt, so the search lands on the switch/select statement itself,
// which is not an assignment, and nothing inside one is ever reported.
func inCaseClause(n int) {
	switch n {
	case 1:
		resp, err := http.Get("http://example.com")
		defer resp.Body.Close()
		_ = err
	}
}

func inCommClause(ch chan int) {
	select {
	case <-ch:
		resp, err := http.Get("http://example.com")
		defer resp.Body.Close()
		_ = err
	}
}
