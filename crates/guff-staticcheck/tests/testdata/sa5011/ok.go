package main

func ok() {
	var v int
	y := &v
	_ = *y
}

// Nil-check on a map behind a pointer must not flag the pointer deref used to
// inspect/assign the map (prometheus Annotations pattern).
type M map[string]int

func okMapPtr(a *M) {
	if *a == nil {
		*a = M{}
	}
	(*a)["k"] = 1
}

// Multiple nil-checks on the same pointer must not overwrite each other
// (caddy respHeaderOps: early `!= nil` use, later `== nil` return).
type H struct{ Deferred bool }

func okMultiCheck(p *H) {
	if p != nil {
		p.Deferred = true
	}
	if p == nil {
		return
	}
	p.Deferred = false
}

// Short-circuit `&&` nil guards (caddy acmeIssuer.Challenges.DNS).
type DNS struct{}
type Challenges struct{ DNS *DNS }
type Issuer struct{ Challenges *Challenges }

func okShortCircuit(a *Issuer) *DNS {
	if a != nil && a.Challenges != nil && a.Challenges.DNS != nil {
		return a.Challenges.DNS
	}
	return nil
}

// Captured err FreeVar after nil-check must not flag later Store (containerd
// GIDFromFS pattern: gid, err = ... inside a closure that closed over err).
func okCapturedErr() {
	var err error
	f := func() {
		if err != nil {
			return
		}
		err = nil
	}
	f()
}
