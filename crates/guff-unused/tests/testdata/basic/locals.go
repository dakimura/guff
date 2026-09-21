package locals

// Function-local declarations.
//
// honnef's `seeScope` sees every object in every scope, and `g.stmt`'s
// `*ast.DeclStmt` arm calls the same `g.decl` the package level uses — so a
// type or constant declared inside a function body is a candidate like any
// other. Only *variables* are exempt (`LocalVariablesAreUsed`, on by default,
// marks every non-field `*types.Var` used). guff's `unused` was
// package-level-only, so none of the `// FINDING` lines below were reported.
//
// Every mark here was measured against golangci-lint 2.12.2, and
// `compat/golden/cases/unused` runs both tools over this same file.

// `Use` is exported, so it is a root; the fixture imports nothing because the
// unit-test harness type-checks it against no other package.
func Use(v ...any) {}

func localTypeUnused() { // FINDING func localTypeUnused (nothing calls it)
	type quiet struct{} // silent — owned by an unused function
	const alsoQuiet = 1 // silent — same
	Use("x")
}

func LocalTypeUnused() {
	type io struct{} // FINDING type io (beats' readjson/json_test.go)
	Use("x")
}

func LocalTypeUsed() {
	type used struct{ n int }
	Use(used{1}) // a positional literal writes `n`, so neither reports
}

func LocalConstUnused() {
	const c = 1 // FINDING const c
	Use("y")
}

func LocalTypeInMethodAndLit() {
	type inFunc struct{} // FINDING type inFunc
	func() {
		type deep struct{} // FINDING type deep
		Use("w")
	}()
}

// `recv` is only ever a receiver, so the type and its exported method are both
// unused — and `inner`, owned by an unused method, is quiet.
type recv struct{} // FINDING type recv

func (recv) M() { // FINDING func recv.M
	type inner struct{} // silent — owned by an unused method
	Use("z")
}

func LocalIfaceUnused() {
	type iface interface{ Do() } // FINDING type iface
	Use("v")
}

// An exported *name* is not global, so the exported-is-used rule (gated on
// `isGlobal`) does not apply to it.
func ExportedLocalName() {
	type Exported struct{} // FINDING type Exported
	Use("exported")
}

// (9.9) an object named the blank identifier is used.
func BlankLocalType() {
	type _ struct{} // silent
	Use("blank")
}

// (10.1) if one constant of a group is used, the whole group is.
func LocalConstGroup() {
	const (
		one = 1 // silent
		two = 2 // silent — kept alive by `one`
	)
	Use(one)
}

func LocalConstGroupDead() {
	const (
		three = 3 // FINDING const three
		four  = 4 // FINDING const four
	)
	Use("none")
}

// A local type's body belongs to *it*, not to the function: `holder` does not
// keep `leaf` alive, because `holder` is unused too.
func ChainedLocalTypes() {
	type leaf struct{ n int }    // FINDING type leaf
	type holder struct{ l leaf } // FINDING type holder
	Use("chain")
}

// Fields of a local struct follow the ordinary field rules once the type
// itself is used.
func LocalTypeFields() {
	type withFields struct {
		usedField   int
		unusedField int // FINDING field unusedField
	}
	v := withFields{usedField: 1}
	Use(v.usedField)
}

// Nested blocks are scopes too.
func NestedBlocks(c bool) {
	if c {
		type inIf struct{} // FINDING type inIf
		Use("if")
	}
	switch {
	case c:
		type inCase struct{} // FINDING type inCase
		Use("case")
	}
}

// A local alias and a local generic type.
func LocalAliasAndGeneric() {
	type alias = int              // FINDING type alias
	type box[T any] struct{ v T } // FINDING type box
	Use("alias")
}

// `//lint:ignore U1000` over a local declaration reaches *that* declaration:
// `ast.NewCommentMap` associates a directive inside a body with the statement
// below it, not with the next top-level declaration.
func Ignored() {
	//lint:ignore U1000 kept on purpose
	type ignored struct{} // silent
	Use("ignored")
}
