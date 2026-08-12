// Package dotimport has two files that dot-import the same package, and both
// use it. Go's unused-import check is per file, so neither import is unused.
//
// guff kept one `dotImportMap` entry per *package* rather than per
// (file scope, name), so the file whose PkgName lost the race was reported as
// "imported and not used" — an error that makes the whole package ill-typed,
// which silently drops every type-dependent finding in it. Nothing in the
// finding set says so; only the count in compat/baselines/health.json moves.
package dotimport

import . "example.com/dotimport/lib"

func One() string {
	s := Hello()
	return s
}
