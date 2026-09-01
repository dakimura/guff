// The far end of a dot import: a package-level variable another package
// assigns without a qualifier.
package shared

type Report struct{ N int }

// Shared is assigned through a dot import by the package next door.
var Shared *Report
