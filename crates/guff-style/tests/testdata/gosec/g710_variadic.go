// Package gosec_g710_variadic pins the one place gosec's taint engine drops a
// flow it can otherwise see: the arguments go/ssa packs into a variadic array.
//
// The callee summary (`doTaintedArgsFlowToReturn` →
// `valueReachableFromParams`) asks whether a tainted parameter reaches a
// `return`. Its `*ssa.Alloc` arm follows direct stores and `FieldAddr` stores
// and nothing else — not `IndexAddr`, which is the only way into the
// `new [N]T (varargs)` go/ssa builds for a non-spread variadic call.
// (`isTainted`, the main walk, *does* follow `IndexAddr`; only the summary
// does not.) guff never builds that array — it passes the tail through as
// ordinary arguments — so without subtracting the tail back out it sees flows
// upstream has lost. kratos's `SecureRedirectTo`, whose result comes from
// `url.Parse(cmp.Or(o.returnTo, source.Query().Get("return_to")))`, was the
// one that showed it.
//
// Every function is marked `// fires` or `// silent`, measured against
// golangci-lint 2.12.2 (gosec v2.27.1).
package gosec_g710_variadic

import (
	"net/http"
	"net/url"
)

func or(vals ...string) string {
	for _, v := range vals {
		if v != "" {
			return v
		}
	}
	return ""
}

func firstOf(a, b string) string {
	if a != "" {
		return a
	}
	return b
}

func prefixed(prefix string, vals ...string) string {
	return prefix + or(vals...)
}

type joiner interface{ join(vals ...string) string }

type impl struct{}

func (impl) join(vals ...string) string { return or(vals...) }

type helper struct{}

func (helper) pick(vals ...string) string { return or(vals...) }

// 1 plain: the tainted string is an ordinary argument.
func plain(r *http.Request) *url.URL {
	u, _ := url.Parse(firstOf("", r.URL.Query().Get("return_to")))
	return u
}

// 2 variadic tail: go/ssa packs it into an array the summary walk cannot see.
func variadicTail(r *http.Request) *url.URL {
	u, _ := url.Parse(or("", r.URL.Query().Get("return_to")))
	return u
}

// 3 variadic, but the taint is in the FIXED parameter.
func variadicFixed(r *http.Request) *url.URL {
	u, _ := url.Parse(prefixed(r.URL.Query().Get("return_to"), "x"))
	return u
}

// 4 spread: the slice is passed straight through, and is opaque either way.
func spread(r *http.Request) *url.URL {
	all := []string{"", r.URL.Query().Get("return_to")}
	u, _ := url.Parse(or(all...))
	return u
}

// 5 a variadic method on a concrete receiver.
func methodTail(r *http.Request) *url.URL {
	u, _ := url.Parse(helper{}.pick("", r.URL.Query().Get("return_to")))
	return u
}

// 6 a variadic method through an interface (SSA invoke).
func invokeTail(r *http.Request, j joiner) *url.URL {
	u, _ := url.Parse(j.join("", r.URL.Query().Get("return_to")))
	return u
}

// 7 no helper at all.
func none(r *http.Request) *url.URL { return r.URL }

func S1(w http.ResponseWriter, r *http.Request) { http.Redirect(w, r, plain(r).String(), 303) } // fires
func S2(w http.ResponseWriter, r *http.Request) { http.Redirect(w, r, variadicTail(r).String(), 303) } // silent
func S3(w http.ResponseWriter, r *http.Request) { http.Redirect(w, r, variadicFixed(r).String(), 303) } // fires
func S4(w http.ResponseWriter, r *http.Request) { http.Redirect(w, r, spread(r).String(), 303) } // silent
func S5(w http.ResponseWriter, r *http.Request) { http.Redirect(w, r, methodTail(r).String(), 303) } // silent
func S6(w http.ResponseWriter, r *http.Request, j joiner) {
	http.Redirect(w, r, invokeTail(r, j).String(), 303) // silent
}
func S7(w http.ResponseWriter, r *http.Request) { http.Redirect(w, r, none(r).String(), 303) } // fires
