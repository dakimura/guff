// A package comment that does not begin with "Package presetdoc": revive's
// package-comments rule reports it in the "should be of the form" shape, which
// is the one EXC0013 names. It needs a package of its own, because the shape
// EXC0015 reports (no package comment at all) is mutually exclusive with it.
package presetdoc

func mkerr() error { return nil }

// Run keeps the package from being empty.
func Run() {
	_ = mkerr()
}
