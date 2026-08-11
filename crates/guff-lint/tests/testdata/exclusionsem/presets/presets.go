// This comment is deliberately separated from the package clause by a blank
// line, so it is not a package doc comment: the package therefore has none,
// which is what ST1000 (EXC0011) and revive's package-comments (EXC0015)
// report on.
//
// Undocumented below is EXC0012 ("should have comment or be unexported") and
// so it cannot be given one; every other function names its rule in its own
// doc comment.

package presets

import (
	"os"
	"os/exec"
	"unsafe"
)

// A doc comment that does not start with the function's name: revive's
// exported rule reports it in the "should be of the form" shape (EXC0014).
func Exported() {}

func Undocumented() {}

// Mkdir passes a permission wider than 0750 (gosec G301, EXC0009).
func Mkdir(dir string) {
	_ = os.Mkdir(dir, 0o777)
}

// Exec launches a subprocess named by a variable (gosec G204, EXC0007). The
// variable has to be a local: gosec skips a *parameter* in the executable-name
// slot, because a parameter is declared before the enclosing body's brace.
func Exec() error {
	name := os.Getenv("GUFF_FIXTURE_CMD")
	return exec.Command(name).Run()
}

// Ptr converts an integer to a pointer (govet unsafeptr, EXC0004; gosec G103,
// EXC0006).
func Ptr(x uintptr) *int {
	return (*int)(unsafe.Pointer(x))
}

// Loop breaks out of the switch rather than the loop (staticcheck SA4011,
// EXC0005).
func Loop(xs []int) {
	for _, x := range xs {
		switch x {
		case 1:
			break
		}
	}
}
