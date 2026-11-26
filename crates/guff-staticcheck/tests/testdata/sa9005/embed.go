package sa9005embed

// Which structs SA9005 calls field-less. Upstream dereferences the argument
// (`typeutil.Dereference`) and looks at `typeutil.FlattenFields`: an embedded
// struct — or pointer to one — is replaced by its own fields, recursively, so
// the embedded field's *name* never counts. Every shape was measured against
// golangci-lint 2.12.2; the FINDING ones are the only reports.

import "encoding/json"

type onlyHidden struct{ a, b int }

type OnlyHidden struct{ a int }

type Visible struct{ A int }

type ch chan int

type Rec struct {
	*Rec
	x int
}

// 1. plain: no exported field — FINDING
func Plain() { _, _ = json.Marshal(onlyHidden{}) }

// 2. through a pointer: Dereference first — FINDING
func Pointer() { _, _ = json.Marshal(&onlyHidden{}) }

// 3. an unexported embedded struct with an exported field
func UnexportedEmbedsExported() { _, _ = json.Marshal(struct{ inner }{}) }

type inner struct{ Method int }

// 4. an *exported* embedded struct with only unexported fields — FINDING
func ExportedEmbedsHidden() { _, _ = json.Marshal(struct{ OnlyHidden }{}) }

// 5. same, embedded by pointer — FINDING
func ExportedEmbedsHiddenPtr() { _, _ = json.Marshal(struct{ *OnlyHidden }{}) }

// 6. an exported embedded struct with an exported field
func ExportedEmbedsVisible() { _, _ = json.Marshal(struct{ Visible }{}) }

// 7. an unexported embedded non-struct — FINDING
func EmbedsChan() { _, _ = json.Marshal(struct{ ch }{}) }

// 8. a self-embedding struct terminates — FINDING
func Recursive() { _, _ = json.Marshal(Rec{}) }

// 9. Unmarshal through a pointer (the usual shape) — FINDING
func Unmarshal(b []byte) { var v onlyHidden; _ = json.Unmarshal(b, &v) }

// 10. a named pointer-to-struct — FINDING
type P *onlyHidden

func NamedPtr(p P) { _, _ = json.Marshal(p) }
