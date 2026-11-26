package pkgshadow

// A package-level func named copy shadows the builtin everywhere in the package.
func copy(a, b []string) {}

var _ = copy

// Silent: `copy` is not the builtin here.
func Loop(dst, src []string) {
	for i := range src {
		dst[i] = src[i]
	}
}
