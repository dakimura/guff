// The builtins whose argument is used only for its type.
//
//	if fun, ok := pass.TypesInfo.Uses[id].(*types.Builtin); ok {
//		switch fun.Name() {
//		case "len", "cap", "Sizeof", "Offsetof", "Alignof":
//			// The argument of this operation is used only
//			// for its type (e.g. len(array)), or the operation
//			// does not copy a lock (e.g. len(slice)).
//			return
//		}
//	}
//
// `fun.Name()` is the builtin's own name — `Sizeof`, never `unsafe.Sizeof`.
// Matching the qualified spelling instead left the three `unsafe` entries dead,
// and `unsafe.Sizeof(*lc)` on a struct holding a lock was a finding
// (VictoriaMetrics `lib/promutil/labelscompressor.go:26`).
package sizeof

import (
	"sync"
	"unsafe"
)

type withMutex struct {
	mu sync.Mutex
	n  int
}

// silent — nothing is evaluated, so nothing is copied.
func sizeofDeref(t *withMutex) uintptr { return unsafe.Sizeof(*t) }

func alignofDeref(t *withMutex) uintptr { return unsafe.Alignof(*t) }

func offsetofField(t *withMutex) uintptr { return unsafe.Offsetof(t.n) }

func lenOfSlice(a []withMutex) int { return len(a) }

// fires — a real copy of the same value.
func copyDeref(t *withMutex) withMutex { return *t }
