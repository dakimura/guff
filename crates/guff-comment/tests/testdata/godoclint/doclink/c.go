package doclink

import (
	coll "encoding/json"
	_ "sort"
)

var _ = coll.Encoder{}

// November refers to coll.Encoder. The alias is bound to two different import
// paths across this package (see d.go), and go doc resolves package names
// package-wide, so such a link would render as plain text — upstream skips it.
const November = 0

// Oscar refers to sort.Ints, which this file imports for side effects only.
// A `_` import contributes no name, but "sort" is still resolved as a bare
// path, so this one is reported.
const Oscar = 0
