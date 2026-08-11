//nolint:errcheck

// A blank line between the directive and the package clause: the comment group
// ends two lines above `package`, so `r.To == nodeStartLine-1` is false and the
// file is *not* covered.
package gap

func mkerr() error { return nil }

func A() {
	mkerr()
}
