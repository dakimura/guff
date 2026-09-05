package net

import "time"

type Listener interface {
	Close() error
}

func Listen(network, address string) (Listener, error) { return nil, nil }

type Conn interface {
	Close() error
}

func Dial(network, address string) (Conn, error) { return nil, nil }

func DialTimeout(network, address string, timeout time.Duration) (Conn, error) { return nil, nil }

func LookupHost(host string) ([]string, error) { return nil, nil }
