// Not upstream: exactly five exported struct types, plus every shape
// revive v1.17.1 does not count, so the default limit (5) is not exceeded.
package five

type A struct{}
type B struct{}
type C struct{}
type D struct{}
type E struct{}

type I interface{}  // not a struct
type N int          // not a struct
type P (struct{})   // a parenthesized struct is not *ast.StructType
type _u struct{}    // not exported
type lower struct{} // not exported
