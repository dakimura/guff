package sub

import "fmt"

// ExportedWrapf is a printf wrapper declared in a *dependency*. Upstream
// exports an object fact for it and the importer checks the call; guff
// analyses only the packages being linted, so there is no fact to import and
// the call goes unchecked. Measured, and left silent — see the DEFERRED note
// in `printf_wrappers`.
func ExportedWrapf(format string, args ...any) {
	fmt.Printf(format, args...)
}

// ExportedNotAWrapper has a wrapper's signature and forwards nothing.
func ExportedNotAWrapper(format string, args ...any) {
	_ = format
	_ = args
}
