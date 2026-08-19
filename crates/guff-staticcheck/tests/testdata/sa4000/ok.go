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
}
