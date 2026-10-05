package foo

import _ "unsafe"

var DoWork func()

func regularFunc(f int) { DoWork() }

//go:linkname linknamed other/pkg.linknamed
func linknamed(f int) { DoWork() }
