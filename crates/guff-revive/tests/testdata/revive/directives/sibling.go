package directives

// DirectivesSibling is in a *second* file. The block directive in
// directives.go must not reach it — upstream computes the intervals one file at
// a time, and building them per package silences whatever line happens to fall
// in the same range here.
type DirectivesSibling struct{ ID int64 }
