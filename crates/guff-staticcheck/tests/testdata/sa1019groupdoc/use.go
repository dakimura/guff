// Package depr uses every name dep declares, deprecated or not, so the golden
// records both halves: the names upstream reports and the names it stays
// silent about. A defect that widens the deprecated set shows up as extra
// findings here rather than as nothing at all.
package depr

import "example.com/sa1019groupdoc/dep"

var (
	_ = dep.KindA
	_ = dep.KindB
	_ = dep.KindC
	_ = dep.KindD
	_ = dep.KindE
	_ dep.Alpha
	_ dep.Beta
	_ dep.Gamma
	_ = dep.VarA
	_ = dep.VarB
	_ = dep.VarC
)

func Use() {
	dep.OldThing()
	dep.NewThing()
}
