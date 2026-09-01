package net

type Listener interface {
	Close() error
}

func Listen(network, address string) (Listener, error) { return nil, nil }

type Conn interface {
	Close() error
}

func Dial(network, address string) (Conn, error) { return nil, nil }
