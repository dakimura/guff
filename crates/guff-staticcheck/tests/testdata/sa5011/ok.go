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
