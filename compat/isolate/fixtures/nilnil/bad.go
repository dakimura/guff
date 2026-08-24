package p

// nilnil reports `return nil, nil` for several checked kinds, and the kind is
// named in the message — so each kind is its own arm, not a repeat.

type User struct{}

func Ptr() (*User, error) {
	return nil, nil
}

func Map() (map[string]int, error) {
	return nil, nil
}

func Chan() (chan int, error) {
	return nil, nil
}

func Func() (func(), error) {
	return nil, nil
}

func Iface() (any, error) {
	return nil, nil
}

func Uintptr() (uintptr, error) {
	return 0, nil
}
