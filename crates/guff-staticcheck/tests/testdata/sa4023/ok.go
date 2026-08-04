package main

type iface interface{ M() }
type concrete struct{}

func (*concrete) M() {}

func returns() error {
	var err error
	return err
}

func get() (iface, bool) {
	var m map[int]iface
	v, ok := m[0]
	return v, ok
}

// Comparing an interface from a call/map lookup to nil is valid even if the
// same variable is later assigned a concrete pointer (go-redis Manager.Listener).
func reuseListener() iface {
	listener, ok := get()
	if !ok || listener == nil {
		newCredListener := &concrete{}
		listener = newCredListener
	}
	return listener
}

func main() {
	_ = returns() == nil
	_ = reuseListener()
}
