package bodyclose

import (
	"io"
	"net/http"
)

// Upstream decides with `isopen`, a walk over the response value's SSA
// referrers. guff's port is an AST approximation, and these are the shapes
// where the two used to part company. Each arm below names the upstream
// branch it stands for.

// HTTPClient is connect-go's interface over *http.Client.
type HTTPClient interface {
	Do(*http.Request) (*http.Response, error)
}

type holder struct {
	client   HTTPClient
	response *http.Response
}

var global *http.Response

// `getReqCall` accepts *any* call whose result mentions `*http.Response`, so a
// helper of this package is a candidate too — and a result nobody binds has no
// referrers, which `isopen` answers "open" for. Reported at the helper call.
func fetch(client *http.Client, req *http.Request) *http.Response {
	response, _ := client.Do(req)

	return response
}

func readsAField(client *http.Client, req *http.Request) bool {
	return fetch(client, req).Uncompressed
}

// `*ssa.Store` into a `*ssa.Global`: "Referrers for globals are always nil, so
// skip". Silent.
func intoGlobal(client *http.Client, req *http.Request) {
	response, err := client.Do(req)
	if err != nil {
		return
	}
	global = response
}

// `*ssa.Store` into a `*ssa.FieldAddr`, settled by a close on the body reached
// through that field. Silent.
func (h *holder) intoFieldClosed(req *http.Request) {
	response, err := h.client.Do(req)
	if err != nil {
		return
	}
	h.response = response
	_ = h.response.Body.Close()
}

// The same store with no such close. Reported.
func (h *holder) intoField(req *http.Request) {
	response, err := h.client.Do(req)
	if err != nil {
		return
	}
	h.response = response
}

// `*ssa.MakeClosure`: a response captured by a func literal is settled however
// the literal uses it — draining the body, reading a field, or nothing at all.
// All three silent.
func deferDrains(client *http.Client, req *http.Request) {
	response, err := client.Do(req)
	if err != nil {
		return
	}
	defer func() {
		_, _ = io.Copy(io.Discard, response.Body)
	}()
}

func deferTouchesNothing(client *http.Client, req *http.Request) {
	response, err := client.Do(req)
	if err != nil {
		return
	}
	defer func() {
		_ = response.StatusCode
	}()
}

// Draining inline, with no closure, is *not* settled. Reported.
func drainedInline(client *http.Client, req *http.Request) error {
	response, err := client.Do(req)
	if err != nil {
		return err
	}
	_, err = io.Copy(io.Discard, response.Body)

	return err
}

// The `*ssa.Call` arm walks into a static callee and answers "not open" only
// on finding a close there. Handing the response to something that merely
// reads it settles nothing.
func closesIt(response *http.Response) { _ = response.Body.Close() }

func validates(response *http.Response) bool { return response.StatusCode == http.StatusOK }

// Silent: the callee closes.
func passedToCloser(client *http.Client, req *http.Request) {
	response, err := client.Do(req)
	if err != nil {
		return
	}
	closesIt(response)
}

// Reported: the callee only reads.
func passedToReader(client *http.Client, req *http.Request) {
	response, err := client.Do(req)
	if err != nil {
		return
	}
	_ = validates(response)
}
