package net

type Listener interface {
	Close() error
}

func Listen(network, address string) (Listener, error) { return nil, nil }
