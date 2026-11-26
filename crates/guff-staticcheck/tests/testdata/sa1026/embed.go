package sa1026embed

// How SA1026 walks a struct: upstream's fakejson is a copy of encoding/json's
// `typeFields`, so it keeps json's rules for embedded fields — a breadth-first
// walk over the embedded structs, then one survivor per name: the shallowest,
// a tagged one over an untagged one at the same depth, and none at all when
// two tie. A field json never encodes cannot make the value unmarshalable.
//
// beats' `Config.MarshalJSON` is shape 1: it marshals
// `struct{ Method string; *Alias }`, whose `Alias.Method` (a func inside) is
// hidden by the outer `Method`. Every shape below was measured against
// golangci-lint 2.12.2; the ones marked FINDING are the only reports.

import "encoding/json"

type fn func()

type Inner struct {
	Method fn
	Other  string
}

// 1. an outer field shadows the embedded pointer's field (beats)
func ShadowPtr(in *Inner) {
	_, _ = json.Marshal(&struct {
		Method string
		*Inner
	}{Inner: in})
}

// 2. same, embedded by value
func ShadowVal(in Inner) {
	_, _ = json.Marshal(struct {
		Method string
		Inner
	}{Inner: in})
}

// 3. not shadowed: reported through the embedding — FINDING
func NotShadowed(in Inner) {
	_, _ = json.Marshal(struct {
		Name string
		Inner
	}{Inner: in})
}

type A struct{ Method fn }
type B struct{ Method string }

// 4. two embeddings at the same depth: the name annihilates
func Annihilate() {
	_, _ = json.Marshal(struct {
		A
		B
	}{})
}

type BT struct {
	Method string `json:"Method"`
}

// 5. a tagged field at the same depth dominates the untagged one
func TagWins() {
	_, _ = json.Marshal(struct {
		A
		BT
	}{})
}

type AT struct {
	Method fn `json:"Method"`
}

// 6. …and a tagged bad field dominates the untagged good one — FINDING
func TagWinsBad() {
	_, _ = json.Marshal(struct {
		AT
		B
	}{})
}

// 7. a deeper field is hidden by a shallower one under a *tag* name
func TagShadows(in Inner) {
	_, _ = json.Marshal(struct {
		X string `json:"Method"`
		Inner
	}{Inner: in})
}

type ch chan int

// 8. an unexported embedded non-struct is ignored
func UnexportedEmbedded() {
	_, _ = json.Marshal(struct {
		ch
		Name string
	}{})
}

// 9. an embedded struct with a json name is a field, not flattened — FINDING
func TaggedEmbedded(in Inner) {
	_, _ = json.Marshal(struct {
		Inner `json:"inner"`
	}{Inner: in})
}

// 10. `json:"-"` on the embedding
func DashEmbedded(in Inner) {
	_, _ = json.Marshal(struct {
		Inner `json:"-"`
	}{Inner: in})
}

type Mid1 struct{ A }
type Mid2 struct{ A }

// 11. the same struct reached twice at depth 2: its fields annihilate
func TwicePromoted() {
	_, _ = json.Marshal(struct {
		Mid1
		Mid2
	}{})
}

type PM struct{ F fn }

func (*PM) MarshalJSON() ([]byte, error) { return nil, nil }

// 12. embedded by pointer: addressable, so the pointer-receiver MarshalJSON counts
func PtrRecvViaPtr() {
	_, _ = json.Marshal(struct{ *PM }{})
}

// 13. embedded by value in a value: not addressable, so it is walked — FINDING
func PtrRecvViaVal() {
	_, _ = json.Marshal(struct{ PM }{})
}

// 14. `json:"-,"` names the field "-"; it is encoded — FINDING
func DashComma() {
	_, _ = json.Marshal(struct {
		F fn `json:"-,"`
	}{})
}

type inner struct{ Method fn }

// 15. an unexported embedded struct still contributes its exported fields — FINDING
func UnexportedStruct() {
	_, _ = json.Marshal(struct{ inner }{})
}
