package main

// The union terms of a type parameter constraint are `*ast.BinaryExpr` with a
// `|` operator, so SA4000 walks straight into them. What keeps it quiet is the
// render comparison: two `*ast.FuncType` are only identical if they print the
// same. gitea writes `func(EngineMigration) error | func(context.Context,
// EngineMigration) error`; the shape is what matters, not the arguments.
type migrate[T func(int) error | func(int, int) error] struct{}

type writerOf[F func(int) string | func(int) (string, error)] struct{}

func main() {
	x, y := 1, 2
	_ = x == y
	// Distinct composite lits must not collapse to identical `<expr>` renders.
	type id struct{ Name, Group, Kind string }
	a, b := id{Name: "dash-1", Group: "g", Kind: "K"}, id{Name: "dash-2", Group: "g", Kind: "K"}
	_ = a.Name == b.Name && a.Group == b.Group
	// Two different func literals: another kind the old renderer flattened.
	f, g := func(int) {}, func(string) {}
	_, _ = f, g
	var _ migrate[func(int) error]
	var _ writerOf[func(int) string]
	// `isFloat` is asked of the type's **term set**, and recurses into arrays
	// and structs: `==` on a `[2]float64` or on a `struct{ f float64 }` is
	// legal and meaningful (NaN), so upstream skips both. A type parameter
	// whose set has no terms is skipped for the same reason — "no terms, so
	// floats are a possibility". guff looked for a basic float only and
	// reported all three.
	var af arr
	_ = af == af
	var sf st
	_ = sf == sf
	_ = tpEq(1)
	var fl float64
	_ = fl - fl
}

type arr [2]float64

type st struct{ f float64 }

func tpEq[T comparable](x T) bool { return x == x }
