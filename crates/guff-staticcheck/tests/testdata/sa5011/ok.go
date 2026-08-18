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
