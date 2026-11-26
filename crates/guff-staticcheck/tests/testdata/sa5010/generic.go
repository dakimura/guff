package main

// SA5010 over type parameters.
//
// Upstream's escape hatch is the *method*, not the type:
//
//	if ml.Origin() != ml || mr.Origin() != mr {
//		// Give up when we see generics.
//		continue instrLoop
//	}
//
// `Origin()` is the method as the source declares it, so any method that
// instantiation cloned takes the whole assertion out of the check — including
// `Merge[int, string]` vs `Merge[string, int]`, where no type parameter is
// free and govet's `ifaceassert` still reports (its own hatch is
// `free.Has`, a different question). pyroscope writes sixteen of these.

type gMerge[Res any, Req any] interface {
	Send(Res) error
	Receive() (Req, error)
}

// silent — the methods of `gMerge[Res, Req]` are clones.
func gSwitch[B gMerge[Res, Req], Res any, Req any](stream B) {
	switch t := gMerge[Res, Req](stream).(type) {
	case gMerge[int, string]:
		_ = t
	case gMerge[string, int]:
		_ = t
	}
}

// silent — the same, as a plain assertion.
func gAssert[B gMerge[Res, Req], Res any, Req any](stream B) {
	_ = gMerge[Res, Req](stream).(gMerge[int, string])
}

// silent — still clones, even with every type argument concrete. This is the
// shape that says the test is `Origin()` and not "has a free type parameter".
func gConcrete(s gMerge[int, string]) {
	_ = s.(gMerge[string, int])
}

// fires — no generics anywhere.
type gPlainA interface{ Get() int }
type gPlainB interface{ Get() string }

func gPlain(v gPlainA) {
	_ = v.(gPlainB)
}

// fires — an anonymous interface's methods are never clones, so `Origin()`
// says nothing about the `T` in its signature. govet's `ifaceassert` is silent
// here for the opposite reason: `free.Has` finds that `T`. The two escape
// hatches disagree in both directions, which is why each has its own fixture.
func gNested[T any](x interface{ Get() map[string][]T }) {
	_ = x.(interface{ Get() int })
}
