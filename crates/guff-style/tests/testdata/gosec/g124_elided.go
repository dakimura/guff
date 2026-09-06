// Package gosec_g124_elided fixes the shapes of gosec G124 that decide whether
// upstream's SSA rule has an allocation to name.
//
// gosec keys a cookie by the SSA value its field stores are rooted at and drops
// any root whose Pos() is token.NoPos. That is not the same as "the literal
// spells out its type": an element that ELIDES its type still gets an
// `Alloc (complit)` positioned at the inner `{` whenever the element is a
// pointer, or whenever the container is a map. Only a slice / array **of
// values** roots its stores at an IndexAddr, which carries no position.
//
// Every line below was measured against golangci-lint 2.12.2 (gosec v2.27.1)
// with max-same-issues: 0; `want` marks the ones upstream reports.
package gosec_g124_elided

import "net/http"

type Jar []*http.Cookie

type ValJar []http.Cookie

type Box struct{ Cs []*http.Cookie }

// --- spelled-out types: reported before this fixture existed too ------------

func explicitPointer() *http.Cookie { return &http.Cookie{Name: "a"} } // want

func explicitValue() http.Cookie { return http.Cookie{Name: "g"} } // want

func explicitInSlice() []*http.Cookie { return []*http.Cookie{&http.Cookie{Name: "l"}} } // want

func explicitInStructField() Box { return Box{Cs: []*http.Cookie{&http.Cookie{Name: "m"}}} } // want

func explicitInAny() []any { return []any{&http.Cookie{Name: "x"}} } // want

// --- elided element, pointer: an Alloc at the inner `{` ---------------------

func elidedSliceOfPointers() []*http.Cookie { return []*http.Cookie{{Name: "b"}} } // want

func elidedArrayOfPointers() [1]*http.Cookie { return [1]*http.Cookie{{Name: "r"}} } // want

func elidedMapOfPointers() map[string]*http.Cookie {
	return map[string]*http.Cookie{"d": {Name: "d"}} // want
}

func elidedNamedSliceOfPointers() Jar { return Jar{{Name: "n"}} } // want

func elidedNestedSlices() [][]*http.Cookie { return [][]*http.Cookie{{{Name: "f"}}} } // want

func elidedInStructField() Box { return Box{Cs: []*http.Cookie{{Name: "p"}}} } // want

func elidedBesideNil() []*http.Cookie { return []*http.Cookie{nil, {Name: "w"}} } // want

func elidedInArrayStructField() struct{ A [1]*http.Cookie } {
	return struct{ A [1]*http.Cookie }{A: [1]*http.Cookie{{Name: "y"}}} // want
}

func elidedBehindAddressOf() *[]*http.Cookie { return &[]*http.Cookie{{Name: "z"}} } // want

func elidedInMapOfSlices() map[string][]*http.Cookie {
	return map[string][]*http.Cookie{"a": {{Name: "aa"}}} // want
}

func elidedInCallArgument(f func([]*http.Cookie)) {
	f([]*http.Cookie{{Name: "j", HttpOnly: true}}) // want
}

// --- elided element, value: only a map materialises an allocation -----------

func elidedMapOfValues() map[string]http.Cookie {
	return map[string]http.Cookie{"k": {Name: "k"}} // want
}

func elidedNestedMapsOfValues() map[string]map[string]http.Cookie {
	return map[string]map[string]http.Cookie{"a": {"b": {Name: "ab"}}} // want
}

func elidedSliceOfValues() []http.Cookie { return []http.Cookie{{Name: "c"}} } // silent

func elidedArrayOfValues() [1]http.Cookie { return [1]http.Cookie{{Name: "e"}} } // silent

func elidedNamedSliceOfValues() ValJar { return ValJar{{Name: "o"}} } // silent

// --- the secure cases: an elided literal must still be able to stay quiet ---

func elidedSecure() []*http.Cookie {
	return []*http.Cookie{{Name: "q", Secure: true, HttpOnly: true, SameSite: http.SameSiteLaxMode}}
}

func explicitSecure() *http.Cookie {
	return &http.Cookie{Name: "i", Secure: true, HttpOnly: true, SameSite: http.SameSiteStrictMode}
}

func elidedInsecureSameSite() []*http.Cookie {
	return []*http.Cookie{{Name: "s", Secure: true, HttpOnly: true, SameSite: http.SameSiteNoneMode}} // want
}
