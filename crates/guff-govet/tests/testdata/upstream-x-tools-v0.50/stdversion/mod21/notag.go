// File version is 1.21.
package mod21

import "go/types"

func _() {
	// old package-level type
	var _ types.Info // ok: defined by go1.0

	// new field of older type
	_ = new(types.Info).FileVersions // want `types.Info.FileVersions requires go1.22 or later \(module is go1.21\)`

	// new method of older type
	_ = new(types.Info).PkgNameOf // want `types.\(\*Info\).PkgNameOf requires go1.22 or later \(module is go1.21\)`

	// new package-level type
	var a types.Alias // want `types.Alias requires go1.22 or later \(module is go1.21\)`

	// new method of new type
	a.Underlying() // no diagnostic
}

