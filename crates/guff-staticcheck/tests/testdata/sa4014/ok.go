package main
func main() {
    x := 1
    if x == 1 {
    } else if x == 2 {
    }
}

// Upstream bails on the whole chain when `code.MayHaveSideEffects(cond, nil)`
// is true, so a repeated condition that *reads a channel* or *calls a
// function* is not a duplicate — each evaluation can differ. guff's copy of
// that predicate was `matches!(expr, Expr::CallExpr(_))`, which does not even
// see a call nested inside a comparison, and reported both of these.
func sideEffectChannel(code int, ch chan int) int {
	if code == <-ch {
		return 1
	} else if code == <-ch {
		return 2
	}
	return 0
}

func sideEffectCall(code int, f func() int) int {
	if code == f() {
		return 1
	} else if code == f() {
		return 2
	}
	return 0
}
