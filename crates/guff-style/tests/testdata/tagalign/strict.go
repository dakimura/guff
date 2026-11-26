package tagalign

// `strict` aligns by **key**, not by position.
//
// Upstream collects the group's distinct keys, sorts them the same way it sorts
// the tags, and then walks tags and columns together: a match writes the tag
// padded to the column's width, a miss writes the column as spaces and only
// advances the column (`tagalign.go`'s `Process`). So a field that has no
// `tag1` still starts its `tag2` at the `tag2` column.
//
// guff had the positional (default) path only — its note said "DEFERRED:
// StrictStyle missing-key column padding … No fixture reaches it", and
// ava-labs/avalanchego reaches it: `validatorMetadata` pads a `v1:"true"` out
// to the width of the `v0:"true"` column above it.
//
// Strict is ignored unless `align` and `sort` are both on
// (`if h.style == StrictStyle && (!h.align || !h.sort) { h.style = DefaultStyle }`),
// which is why the same file is also a case for the default style.

// Already aligned the strict way: the missing tag1 column is padded out.
type AlignedStrict struct {
	BothTags   string `tag1:"true" tag2:"true"`
	SingleTag1 string `tag1:"true"`
	SingleTag2 string `            tag2:"true"`
}

// The same group with the padding missing.
type UnpaddedStrict struct {
	BothTags   string `tag1:"true"  tag2:"true"`
	SingleTag1 string `tag1:"true"`
	SingleTag2 string `tag2:"true"`
}

// A missing *middle* column — positional alignment gets this wrong in the
// other direction, pulling `ccc` left into the `bb` column.
type MiddleMissing struct {
	All  string `a:"1" bb:"2" ccc:"3"`
	NoBB string `a:"1"        ccc:"3"`
}

// A key only one field has.
type ExtraKey struct {
	One string `a:"1"`
	Two string `a:"1" zz:"2"`
}
