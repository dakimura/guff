//go:build go1.26

// Package newexprdirective pins when the newexpr fix adds a `//go:fix inline`
// line (x/tools v0.52 newexpr.go: `astutil.Directives(decl.Doc)` with
// Tool "go", Name "fix", Args exactly "inline").
package newexprdirective

//go:fix inline
func annotated(x int) *int { return &x }

// A doc comment line, then the directive.
//
//go:fix inline
func annotatedAfterDoc(x int) *int { return &x }

//go:fix   inline
func extraSpaces(x int) *int { return &x }

//go:fix inline // keep: arguments are not exactly "inline"
func trailingComment(x int) *int { return &x }

//go:fix inline extra
func extraArg(x int) *int { return &x }

/*go:fix inline*/
func blockComment(x int) *int { return &x }

// go:fix inline
func spaceAfterSlashes(x int) *int { return &x }

//go:fix inline

func blankLineBetween(x int) *int { return &x }

//go:fix inlined
func otherName(x int) *int { return &x }

//go:noinline
func otherDirective(x int) *int { return &x }

func bare(x int) *int { return &x }

type T struct{}

//go:fix inline
func (T) method(x int) *int { return &x }

func (T) methodBare(x int) *int { return &x }
