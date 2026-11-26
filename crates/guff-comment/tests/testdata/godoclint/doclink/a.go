// Package doclink is adapted from godoc-lint's testdata/rule/require_stdlib_doclink.
//
// The rule looks for text that *would* be a doc link if it were bracketed, so
// the fixture has to carry both: real doc links, which must stay silent, and
// the bare spellings, which must not.
package doclink

// Alpha has a real doc link to [encoding/json.Encoder], which the printer
// renders with its brackets — so the "(^|\s)pkg.Name" pattern never sees it.
const Alpha = 0

// Bravo mentions encoding/json.Encoder in plain text.
const Bravo = 0

// Charlie mentions encoding/json.Encoder and encoding/json.Encoder twice.
const Charlie = 0

// Delta mentions encoding/json.Encoder and *encoding/json.Encoder, and the
// star is stripped before counting, so these are two instances of one link.
const Delta = 0

// Echo mentions io.PipeWriter.Close, a method rather than a type.
const Echo = 0

// Foxtrot mentions encoding/json.Encoder and bytes.Buffer, two separate
// findings sorted by the text they replace.
const Foxtrot = 0

// Golf has words that look like doc links but are not symbols:
//
// 8 bits are bytes
//
// fmt is a package
//
// there's no such thing as encoding/json.Play
//
// fmt.PRINTLN
//
// encoding/json.encoder
//
// io.PipeWriter.Closer
const Golf = 0

// Hotel keeps its bare spelling inside a code block, which is stripped:
//
//	encoding/json.Encoder
const Hotel = 0

// # A heading mentioning encoding/json.Encoder
//
// India's heading is dropped too — doc links are not picked up in headings.
const India = 0
