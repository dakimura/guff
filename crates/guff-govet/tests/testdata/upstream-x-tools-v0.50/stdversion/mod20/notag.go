// The 1.20 module is before the forward compatibility regime:
// The file's build tag effects selection, but
// not language semantics, so stdversion is silent.

package mod20

import "go/types"

func _() {
	var _ types.Alias
}

