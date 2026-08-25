package modernize

// Cases mirroring golang.org/x/tools/.../modernize rangeint limit invariance.

func rangeOverParam(n int) {
	for i := 0; i < n; i++ { // want
		_ = i
	}
}

func rangeOverConst() {
	const c = 10
	for i := 0; i < c; i++ { // want
		_ = i
	}
}

func rangeOverLocal() {
	limit := 10
	for i := 0; i < limit; i++ { // want
		_ = i
	}
}

func nopeReassignedLimit() {
	k := 3
	for i := 0; i < k; i++ { // nope: k reassigned below (prometheus Series pattern)
		_ = i
	}
	k = 4
	_ = k
}

func nopeIncLimit() {
	incLimit := 10
	incLimit++
	for i := 0; i < incLimit; i++ { // nope
		_ = i
	}
}

func nopeAddrTakenLimit() {
	addrLimit := 10
	for i := 0; i < addrLimit; i++ { // nope
		_ = i
	}
	_ = &addrLimit
}

func nopeAddrTakenLenSlice() {
	var chks []int
	_ = &chks
	for i := 0; i < len(chks); i++ { // nope
		_ = chks[i]
	}
}

func rangeOverLen(slice []int) {
	for i := 0; i < len(slice); i++ { // want
		_ = slice[i]
	}
}

func nopeOuterLoopVar() {
	for outer := 1; outer < 10; outer++ {
		for i := 0; i < outer; i++ { // nope: outer incremented by outer loop
			_ = i
		}
	}
}

// The `for i = 0` spelling: a range loop leaves `i` holding limit-1, so the
// rewrite is only offered when nothing reads `i` afterwards. dapr's
// `pkg/api/http/directmessaging.go` returns its index after the loop.
func nopeIndexReadAfterLoop(n int) int {
	var i int
	for i = 0; i < n; i++ { // nope: i is read below
		_ = i
	}
	return i
}

func assignIndexNotReadAfterLoop(n int) {
	var i int
	for i = 0; i < n; i++ { // want
		_ = i
	}
}

// A read *inside* the loop is not a read after it.
func assignIndexReadInsideLoop(n int) int {
	var i, found int
	for i = 0; i < n; i++ { // want
		if found > 0 {
			return i
		}
		found++
	}
	return -1
}

// The index is not read in the body, so the fix has to drop `i :=` entirely.
// `for i := range n` that never reads `i` is `declared and not used` — the
// rewrite would not compile.
func indexUnused(n int) {
	for i := 0; i < n; i++ { // want
		println("x")
	}
}

// An inner `i` is a different object, so it is not a use of the loop index.
// Name-based matching would call this used and leave `i :=` in place.
func indexShadowedInBody(n int) {
	for i := 0; i < n; i++ { // want
		i := "inner"
		_ = i
	}
}

// `for i = 0` reuses a variable declared elsewhere: there is no declaration to
// drop, so the index survives even when the body never reads it.
func assignIndexUnusedInBody(n int) {
	var i int
	for i = 0; i < n; i++ { // want
		println("x")
	}
}
