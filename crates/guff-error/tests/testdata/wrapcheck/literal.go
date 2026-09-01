package wrapcheck

import "encoding/json"

var marshal func(v any) ([]byte, error)

var sink func() error

func countOnly() int { return 0 }

// identInLiteral: upstream consults the parent stack only in the *call*
// branch, so the identifier form is reported from inside a func literal.
// `encoding/json` is also the package whose path and name differ, which pins
// the signature's qualifier: go/types renders a nil qualifier as the package
// path, so the message says `func encoding/json.Marshal(...)`.
func identInLiteral() {
	marshal = func(v any) ([]byte, error) {
		b, err := json.Marshal(v)

		return b, err
	}
}

// callInLiteral: a returned call from a literal is skipped.
func callInLiteral() {
	marshal = func(v any) ([]byte, error) {
		return json.Marshal(v)
	}
}

// varDeclInLiteral: a `var` declaration has no assignment statement, so the
// identifier's own declaration is the fallback.
func varDeclInLiteral() {
	marshal = func(v any) ([]byte, error) {
		var b, err = json.Marshal(v)

		return b, err
	}
}

// varDeclInDecl: the same fallback outside a literal.
func varDeclInDecl(v any) ([]byte, error) {
	var b, err = json.Marshal(v)

	return b, err
}

// callBesideErr: a first result that is neither an error nor a tuple ends the
// whole return statement upstream, so the `err` beside it is never examined.
var countAndErr func(v any) (int, error)

func callBesideErr() {
	countAndErr = func(v any) (int, error) {
		_, err := json.Marshal(v)

		return countOnly(), err
	}
}

// nestedLiterals: a literal inside a literal is still a literal.
func nestedLiterals() {
	sink = func() error {
		inner := func() error {
			_, err := json.Marshal(1)

			return err
		}

		return inner()
	}
}

// fromParam: an error that arrives as a parameter has neither an assignment
// nor a var spec, so there is no call to blame.
func fromParam(err error) error {
	return err
}
