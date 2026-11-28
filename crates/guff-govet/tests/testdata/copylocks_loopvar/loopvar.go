package loopvar

import "sync"

type guarded struct {
	mu sync.Mutex
	n  int
}

type nested struct {
	g guarded
}

func zero() sync.Mutex { return sync.Mutex{} }

// Shapes of `for init; cond; post` with a := that declares a lock-holding var.
func shapes(src sync.Mutex, gsrc guarded) {
	// 1. two vars, the second a lock (composite literal rhs is not a copy).
	for i, mu := 0, (sync.Mutex{}); i < 1; i++ {
		_ = &mu
	}
	// 3. no lock among the loop vars.
	for i := 0; i < 1; i++ {
		_ = i
	}
	// 4. a lock alone.
	for mu := (sync.Mutex{}); ; {
		_ = &mu
		break
	}
	// 5. blank lhs is ignored; the named one still reports.
	for _, g := (sync.Mutex{}), (guarded{}); ; {
		_ = &g
		break
	}
	// 6. lock nested inside structs.
	for n := (nested{}); ; {
		_ = &n
		break
	}
	// 7. rhs copies a lock: only the assignment warning, not the loop one.
	for mu := src; ; {
		_ = &mu
		break
	}
	// 7b. one rhs copies, another var holds a lock: lhs = nil drops both.
	for g, mu := gsrc, (sync.Mutex{}); ; {
		_, _ = &g, &mu
		break
	}
	// 8. `=` is not DEFINE.
	var mu2 sync.Mutex
	for mu2 = (sync.Mutex{}); ; {
		_ = &mu2
		break
	}
	// 9. range is the separate range check.
	for _, m := range []sync.Mutex{} {
		_ = &m
	}
	// 10. a call rhs is not a copy, the loop var still is.
	for mu := zero(); ; {
		_ = &mu
		break
	}
	// 11. pointer to lock is fine.
	for p := &src; ; {
		_ = p
		break
	}
	// 12. a := in the body, not the init, is not a loop var.
	for {
		mu := (sync.Mutex{})
		_ = &mu
		break
	}
	// 13. func literal in init with an inner :=; only the outer is the init.
	for f := func() { m := (sync.Mutex{}); _ = &m }; ; {
		f()
		break
	}
}
