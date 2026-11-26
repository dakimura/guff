package gocheckcompilerdirectives

import _ "embed"

// Problematic cases (trailing text after the directive name is required for
// upstream name extraction — bare `//go:name` is skipped):

// go:embed x.txt

//    go:embed y.txt

//go:genrate echo hi

var unused string
