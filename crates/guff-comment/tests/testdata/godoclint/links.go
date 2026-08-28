// Package links exercises godoclint's no-unused-link rule.
//
// The package doc's own definition is used here: [pkgref].
//
// [pkgref]: https://example.com/pkgref
// [pkgunused]: https://example.com/pkgunused
package links

// A documented import block is not a symbol declaration, so upstream never
// puts its doc in the set and this definition is not reported.
//
// [importlink]: https://example.com/import
import "fmt"

// Used references its definition, so nothing is reported here.
//
// See [used] for details.
//
// [used]: https://example.com/used
func Used() { fmt.Print() }

// Unused never references its definition.
//
// [dangling]: https://example.com/dangling
func Unused() {}

// unexported is not exported, and the rule has no visibility filter.
//
// [lower]: https://example.com/lower
func unexported() {}

// Grouped documents the whole block and none of the specs below carry a doc of
// their own, so the parent doc is only reachable from the declaration.
//
// [grouped]: https://example.com/grouped
const (
	A = 1
	B = 2
	C = 3
)

// Both is a parent doc alongside a spec doc; each is checked once.
//
// [parent]: https://example.com/parent
var (
	// D has its own doc.
	//
	// [own]: https://example.com/own
	D = 1

	E = 2
)

// Empty has no specs, so there is no symbol under it and no doc in the set.
//
// [nospecs]: https://example.com/nospecs
const ()

var _ = unexported
