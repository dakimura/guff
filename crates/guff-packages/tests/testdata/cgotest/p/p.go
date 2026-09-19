// Package p uses cgo, which is what puts it on the `go list -compiled` path.
package p

/*
#include <stdlib.h>
*/
import "C"

// Size crosses the cgo boundary so this really is a cgo package.
func Size() int { return int(C.int(1)) }

// Name is here for the test file to call.
func Name() string { return "p" }
