package gosec_g119

// G119 (securego/gosec `analyzers/redirect_header_propagation.go`).
//
// `http.Client.CheckRedirect` is `func(req *Request, via []*Request) error`. A
// policy that copies the previous request's headers onto the new one carries
// `Authorization`, `Cookie` and friends across an origin change. The rule keys
// entirely on the *signature* — a function with a `*http.Request` parameter and
// a `[]*http.Request` parameter is a redirect policy, whatever it is called and
// wherever it is declared — and reports two things: a store into a `http.Header`
// field reachable from `req`, and a `Set`/`Add` of one of the three sensitive
// names on such a header.
//
// beats writes three of these (the cel, jamf and okta inputs), each as a func
// literal assigned to `client.CheckRedirect`.
//
// Each line is marked `// FINDING` or `// silent`, and the golden case
// (compat/golden/cases/gosec) runs both tools over this same file.

import "net/http"

// The beats shape.
func newClient() *http.Client {
	c := &http.Client{}
	c.CheckRedirect = func(req *http.Request, via []*http.Request) error {
		if len(via) == 0 {
			return nil
		}
		prev := via[len(via)-1]
		req.Header = prev.Header.Clone() // FINDING copy
		return nil
	}
	return c
}

// A named function with the same signature is a policy too — the rule never
// looks at who assigns it.
func policy(req *http.Request, via []*http.Request) error {
	req.Header = via[0].Header // FINDING copy
	return nil
}

// A method.
type client struct{ h *http.Client }

func (c *client) redirect(req *http.Request, via []*http.Request) error {
	req.Header = via[0].Header // FINDING copy
	return nil
}

// The parameters in the other order, and extra parameters around them: the two
// questions upstream asks are independent.
func swapped(via []*http.Request, req *http.Request) error {
	req.Header = via[0].Header // FINDING copy
	return nil
}

func extraParams(n int, req *http.Request, m int, via []*http.Request) error {
	req.Header = via[0].Header // FINDING copy
	return nil
}

// The three sensitive names, case-insensitively.
func readd(req *http.Request, via []*http.Request, name string) error {
	req.Header.Set("Authorization", "x")       // FINDING readd
	req.Header.Add("Cookie", "y")              // FINDING readd
	req.Header.Set("cookie", "y")              // FINDING readd
	req.Header.Set("PROXY-AUTHORIZATION", "z") // FINDING readd
	req.Header.Set("X-Custom", "z")            // silent — not sensitive
	req.Header.Set(name, "z")                  // silent — not a constant
	req.Header.Del("Authorization")            // silent — Del is not a mutation gosec tracks
	return nil
}

// The header goes through a local first; the dependency walk follows it.
func viaLocal(req *http.Request, via []*http.Request) error {
	h := req.Header
	h.Set("Authorization", "x") // FINDING readd
	return nil
}

// Not a policy: no `via`.
func notAPolicy(req *http.Request) error {
	req.Header = http.Header{}           // silent
	req.Header.Set("Authorization", "x") // silent
	return nil
}

// Not a policy: no request.
func onlyVia(via []*http.Request) error {
	_ = via
	return nil
}

// A policy that stores into something other than a header, and one whose header
// does not come from `req`.
func otherField(req *http.Request, via []*http.Request) error {
	req.Method = "GET" // silent
	return nil
}

func unrelatedHeader(req *http.Request, via []*http.Request, h http.Header) error {
	h.Set("Authorization", "x") // silent
	return nil
}
