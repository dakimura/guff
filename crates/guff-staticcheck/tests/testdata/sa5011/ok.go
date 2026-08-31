package main

func ok() {
	var v int
	y := &v
	_ = *y
}

type M map[string]int

func okMapPtr(a *M) {
	if *a == nil {
		*a = M{}
	}
	(*a)["k"] = 1
}

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

type DNS struct{}
type Challenges struct{ DNS *DNS }
type Issuer struct{ Challenges *Challenges }

func okShortCircuit(a *Issuer) *DNS {
	if a != nil && a.Challenges != nil && a.Challenges.DNS != nil {
		return a.Challenges.DNS
	}
	return nil
}

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

type R struct {
	Data map[string]interface{}
}

type Status struct {
	Complete bool
}

type TB interface {
	Fatal(args ...interface{})
	Fatalf(format string, args ...interface{})
}

func okOrFatal(t TB, resp *R) {
	if resp == nil || resp.Data == nil {
		t.Fatal("missing")
	}
	_ = resp.Data["config"]
}

func okOrErr(t TB, resp *R, err error) {
	if err != nil || resp == nil {
		t.Fatalf("bad %v %v", resp, err)
	}
	_ = resp.Data["x"]
}

// Concrete value method Fatal is a static call (not interface invoke); sequential
// check stays clean (unlike testing.TB — see bad.go sequentialFatal / vault :414).
type fataler struct{}

func (fataler) Fatal(args ...interface{}) {}

func okSequentialConcreteFatal(t fataler, statusResp *Status) {
	if statusResp == nil {
		t.Fatal("nil")
	}
	_ = statusResp.Complete
}

type Response struct{ StatusCode int }

func get() (*Response, error) { return nil, nil }

func cleanup(*Response) {}

// prometheus web/web_test.go:738. The `if resp != nil` body uses resp, so
// upstream's IR keeps its sigma node and the join below reads a phi — a value
// the nil check was never recorded against.
func okNilCheckThenShortCircuitDeref() bool {
	resp, err := get()
	if resp != nil {
		cleanup(resp)
	}
	return err == nil && resp.StatusCode == 200
}

// `if a || b { … }` and a use below is the shape coredns writes fifteen times
// in test/wildcard_test.go. The first branch decides whether the nil check is
// reached at all, so upstream's IR gives the check's operand a sigma and the
// block below a phi merging it with the other edge — a different `ir.Value`,
// and SA5011 is pure value identity. What the branch body does is beside the
// point: none of these are findings upstream.
func okOrGuardThenUse(fail func(string)) int {
	resp, err := get()
	if err != nil || resp == nil {
		fail("no reply")
	}
	return resp.StatusCode
}

func okOrGuardWithReturn() int {
	resp, err := get()
	if err != nil || resp == nil {
		return 0
	}
	return resp.StatusCode
}

func exits() { panic("no") }

func okOrGuardCallingHelper() int {
	resp, err := get()
	if err != nil || resp == nil {
		exits()
	}
	return resp.StatusCode
}

func getR() (*R2, error) { return nil, nil }

type R2 struct{ Complete bool }

// Deref *first*, then an OR check — also silent, and for the same reason read
// from the other end: the `err != nil` branch decides whether the `p == nil`
// check is reached, so what the check compares is already a renamed value and
// never the one the deref read. The single-check spelling of this
// (`_ = p.F; if p == nil { … }`) *is* reported, and bad.go keeps it.
func derefThenOrCheck() int {
	p, err := getR()
	n := p.Complete
	if err != nil || p == nil {
		return 0
	}
	if n {
		return 1
	}
	return 0
}

type cl struct{ servers []int }

func newCl() *cl              { return nil }
func useAny(...interface{})   {}
func checkFor(f func() error) {}

// The mirror of okOrGuardThenUse, read from the other side: the *deref* sits in
// one arm of a branch and the nil check below the join. Upstream's IR gives the
// arm a sigma (the deref uses it, so it survives pruning) and the join a phi
// merging that sigma with the other edge — so the `BinOp` the check records
// never names the value the deref read.
//
// nats-server jetstream_cluster_3_test.go:8707 is exactly this: `c` is assigned
// and ranged over in the `else` arm, and `if c != nil` comes after the join.
// Verified against honnef ir: deref reads `Sigma c [b0]`, check compares
// `Phi 2:t14 5:t22`.
func okDerefInArmCheckBelowJoin(replicas int) {
	var c *cl
	if replicas == 1 {
		useAny("r1")
	} else {
		c = newCl()
		defer useAny(c)
		for _, s := range c.servers {
			useAny(s)
		}
	}
	if c != nil {
		checkFor(func() error { useAny(c); return nil })
	}
}

// Same, with the pointer assigned *before* the branch.
func okDerefInThenArm(cond bool) {
	c := newCl()
	if cond {
		useAny(c.servers)
	} else {
		useAny("x")
	}
	if c != nil {
		useAny(c)
	}
}

// No else arm: the join *is* a successor of the branch, so the two occurrences
// already sit in different regions.
func okDerefInThenArmNoElse(cond bool) {
	c := newCl()
	if cond {
		useAny(c.servers)
	}
	if c != nil {
		useAny(c)
	}
}

// The pointer is a parameter rather than a local.
func okDerefInArmParam(cond bool, c *cl) {
	if cond {
		useAny(c.servers)
	} else {
		useAny("x")
	}
	if c != nil {
		useAny(c)
	}
}

// A switch is the same branch shape with more than two successors.
func okDerefInSwitchCase(n int) {
	c := newCl()
	switch n {
	case 1:
		useAny(c.servers)
	case 2:
		useAny("x")
	}
	if c != nil {
		useAny(c)
	}
}

// Deref inside a loop body, check after the loop: both are sigmas, of different
// branches.
func okDerefInLoopBody(n int) {
	c := newCl()
	for i := 0; i < n; i++ {
		useAny(c.servers)
	}
	if c != nil {
		useAny(c)
	}
}

// Deref in the then arm, check in the else arm — two sigmas of one branch.
func okDerefAndCheckInSiblingArms(cond bool) {
	c := newCl()
	if cond {
		useAny(c.servers)
	} else if c != nil {
		useAny(c)
	}
}
