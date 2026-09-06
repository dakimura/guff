// Package resultskip pins which functions upstream skips whole.
//
//	FuncLoop:
//	for _, f := range funcs {
//	    // skip if the function is just referenced
//	    for i := 0; i < f.Signature.Results().Len(); i++ {
//	        if f.Signature.Results().At(i).Type().String() == r.resTyp.String() {
//	            continue FuncLoop
//	        }
//	    }
//
// `resTyp` is `*net/http.Response`, and the test is on the resolved type. Four
// leaks below were silenced by answering it on the syntax instead — "a result
// whose type is spelled Response" — and the four after them must stay silent.
package resultskip

import (
	"net/http"

	"example.com/bodyclose/wrap"
)

// Reported: the result type is named Response but belongs to another package.
// boundary's internal/clientcache/internal/client returns (*api.Response, error)
// from both Get and Post, and lost both of its leaks this way.
func otherPackageTypeNamedResponse() (*wrap.Response, error) {
	resp, err := http.Get("https://example.com/skip-1")
	if err != nil {
		return nil, err
	}
	return wrap.New(resp), nil
}

// Reported, and the control for the one above: identical body, a result type
// that is not named Response.
func otherPackageTypeNamedAnythingElse() (*wrap.Wrapper, error) {
	resp, err := http.Get("https://example.com/skip-2")
	if err != nil {
		return nil, err
	}
	return wrap.NewWrapper(resp), nil
}

// Response is this package's own type of that name — no more net/http's than
// an imported one.
type Response struct {
	R *http.Response
}

// Reported: a local type named Response skips nothing either.
func localTypeNamedResponse() (*Response, error) {
	resp, err := http.Get("https://example.com/skip-3")
	if err != nil {
		return nil, err
	}
	return &Response{R: resp}, nil
}

// Reported: upstream compares against the *pointer* type, so a result that is
// an `http.Response` by value skips nothing.
func returnsAResponseByValue() (http.Response, error) {
	resp, err := http.Get("https://example.com/skip-4")
	if err != nil {
		return http.Response{}, err
	}
	_ = resp.StatusCode
	return http.Response{}, nil
}

// Silent: this really does hand a *http.Response to its caller.
func returnsTheResponse() (*http.Response, error) {
	return http.Get("https://example.com/skip-5")
}

// Silent: the skip is by signature, not by dataflow — upstream drops an
// unrelated leak in such a function too, and matching that is the point.
func returnsOneAndLeaksAnother() (*http.Response, error) {
	leaked, err := http.Get("https://example.com/skip-6")
	if err != nil {
		return nil, err
	}
	_ = leaked.StatusCode
	return http.Get("https://example.com/skip-7")
}

// Silent: the response is the second result, as gorilla/websocket's Dial writes
// it. Upstream scans every result, not just the first.
func returnsTheResponseSecond() (error, *http.Response) {
	leaked, err := http.Get("https://example.com/skip-8")
	if err != nil {
		return err, nil
	}
	_ = leaked.StatusCode
	resp, err := http.Get("https://example.com/skip-9")
	return err, resp
}

// Silent: a named result counts the same.
func returnsTheResponseNamed() (out *http.Response, err error) {
	leaked, err := http.Get("https://example.com/skip-10")
	if err != nil {
		return nil, err
	}
	_ = leaked.StatusCode
	out, err = http.Get("https://example.com/skip-11")
	return
}
