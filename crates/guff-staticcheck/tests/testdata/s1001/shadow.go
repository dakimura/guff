package shadow

// S1001 suggests `copy(dst, src)` only where `copy` still names the builtin:
// upstream asks `types.Eval(…, "copy")` at the loop and checks `IsBuiltin()`.
// A local, a parameter or a package-level declaration called `copy` shadows it,
// and the rewrite would not compile — datadog-agent names the result of its
// `DeepCopy` methods `copy` and holds five such loops. The array branch
// suggests an assignment, needs no builtin, and is guarded by nothing.
//
// Measured against golangci-lint 2.12.2.

type T struct{ Cmdline []string }

// 1. plain: reported
func Plain(dst, src []string) {
	for i := range src {
		dst[i] = src[i]
	}
}

// 2. a local named `copy` shadows the builtin: silent (datadog's DeepCopy)
func ShadowedLocal(p *T) *T {
	copy := &T{}
	copy.Cmdline = make([]string, len(p.Cmdline))
	for i := range p.Cmdline {
		copy.Cmdline[i] = p.Cmdline[i]
	}
	return copy
}

// 3. a parameter named `copy`: silent
func ShadowedParam(copy []string, src []string) {
	for i := range src {
		copy[i] = src[i]
	}
}

// 4. the shadow is declared *after* the loop: still the builtin at the loop
func ShadowedAfter(dst, src []string) int {
	for i := range src {
		dst[i] = src[i]
	}
	copy := 1
	return copy
}

// 5. shadowed in an inner block only
func ShadowedInnerBlock(dst, src []string) {
	{
		copy := 2
		_ = copy
	}
	for i := range src {
		dst[i] = src[i]
	}
}

// 6. arrays: the other branch, which has no builtin guard
func ArraysShadowed(dst, src *[4]byte) {
	copy := 3
	_ = copy
	for i := range src {
		dst[i] = src[i]
	}
}
