// Package p is ifaceassert over type parameters.
//
// x/tools bails out before comparing method sets when either side still has a
// free type parameter:
//
//	// Mitigations for interface comparisons and generics.
//	if free.Has(V) || free.Has(T) {
//		return nil
//	}
//
// Without it, an interface instantiated with the enclosing function's own type
// parameters contradicts every instantiation of itself, and each case of a
// type switch inside a generic function is a finding. pyroscope
// (pkg/querier/select_merge.go, pkg/phlaredb/filter_profiles_bidi.go) writes
// sixteen of them.
package p

type Merge[Res any, Req any] interface {
	Send(Res) error
	Receive() (Req, error)
}

// silent — `Res` and `Req` are free here.
func Switch[B Merge[Res, Req], Res any, Req any](stream B) {
	switch t := Merge[Res, Req](stream).(type) {
	case Merge[int, string]:
		_ = t
	case Merge[string, int]:
		_ = t
	}
}

// silent — the same, as a plain assertion.
func Assert[B Merge[Res, Req], Res any, Req any](stream B) {
	_ = Merge[Res, Req](stream).(Merge[int, string])
}

// fires — both sides are instantiated, so nothing is free and the conflict is
// real. (staticcheck's SA5010 gives up here anyway; its escape hatch is the
// method's Origin, not free type parameters. See sa5010's own fixture.)
func Concrete(s Merge[int, string]) {
	_ = s.(Merge[string, int])
}

// silent — the type parameter is three levels down, inside an anonymous
// interface's method signature: Signature → Tuple → Map → Slice → TypeParam.
// `free.Has` walks all of it, so a port that only checks `Named` type
// arguments reports this one. (SA5010 *does* report it: the methods of an
// anonymous interface are not clones, whatever their signatures mention.)
func Nested[T any](x interface{ Get() map[string][]T }) {
	_ = x.(interface{ Get() int })
}

// fires — the same shape with `int` in place of `T`.
func NestedConcrete(x interface{ Get() map[string][]int }) {
	_ = x.(interface{ Get() int })
}
