package p

import (
	"bufio"
	"errors"
	"net/http"
	"os"
)

var readResponse func(r *bufio.Reader, req *http.Request) (*http.Response, error)

var sink func() error

func countOnly() int { return 0 }

// The ident path runs inside a func literal. Upstream consults the parent
// stack only in the *call* branch, so `return resp, err` is reported here even
// though `return http.ReadResponse(r, req)` two functions down is not.
//
// `net/http` is also the only package in this fixture whose path and name
// differ, which is what pins the signature's qualifier: go/types renders a nil
// qualifier as the package *path*, so the message says `*net/http.Request`.
func IdentInLiteral() {
	readResponse = func(r *bufio.Reader, req *http.Request) (*http.Response, error) {
		resp, err := http.ReadResponse(r, req)

		return resp, err
	}
}

// A returned call from a literal is skipped.
func CallInLiteral() {
	readResponse = func(r *bufio.Reader, req *http.Request) (*http.Response, error) {
		return http.ReadResponse(r, req)
	}
}

// So is a returned *method* call.
func MethodCallInLiteral() {
	sink = func() error {
		f, err := os.Open("missing")
		if err != nil {
			return err
		}

		return f.Close()
	}
}

// A `var` declaration has no assignment statement to find, so the identifier's
// own declaration is the fallback. Neither form was reported before.
func VarDeclInLiteral() {
	readResponse = func(r *bufio.Reader, req *http.Request) (*http.Response, error) {
		var resp, err = http.ReadResponse(r, req)

		return resp, err
	}
}

func VarDeclInDecl(r *bufio.Reader, req *http.Request) (*http.Response, error) {
	var resp, err = http.ReadResponse(r, req)

	return resp, err
}

// A first result that is neither an error nor a tuple ends the whole return
// statement upstream: the `err` beside it is never looked at.
var countAndErr func(r *bufio.Reader, req *http.Request) (int, error)

func CallBesideErr() {
	countAndErr = func(r *bufio.Reader, req *http.Request) (int, error) {
		_, err := http.ReadResponse(r, req)

		return countOnly(), err
	}
}

// A literal nested in a literal is still a literal.
func NestedLiterals() {
	sink = func() error {
		inner := func() error {
			f, err := os.Open("missing")
			_ = f

			return err
		}

		return inner()
	}
}

// The most recent assignment before the return decides which call is blamed.
func Reassigned() error {
	err := errors.New("boom")
	if err != nil {
		_ = err
	}
	_, err = os.Open("missing")

	return err
}

// An error that arrives as a parameter has neither an assignment nor a var
// spec, so there is no call to blame.
func FromParam(err error) error {
	return err
}

// `errors.New` is in the default ignore-sigs list, here and inside a literal.
func Ignored() error {
	err := errors.New("x")

	return err
}

func IgnoredVarInLiteral() {
	sink = func() error {
		var err = errors.New("x")

		return err
	}
}
