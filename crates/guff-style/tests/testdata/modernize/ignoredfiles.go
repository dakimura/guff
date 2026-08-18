// Package ignoredfiles has three atomictypes candidates that differ only in
// where they are declared. When the package carries a build-excluded file,
// upstream keeps the local one and drops the other two.
package ignoredfiles

import "sync/atomic"

type roundRobin struct {
	robin uint32
}

func (r *roundRobin) next() uint32 {
	return atomic.AddUint32(&r.robin, 1)
}

var pkgLevel uint32

func bumpPkgLevel() uint32 {
	return atomic.AddUint32(&pkgLevel, 1)
}

func localVar() uint32 {
	var n uint32
	return atomic.AddUint32(&n, 1)
}
