package net

type Conn interface{ Close() error }

func Dial(network, address string) (Conn, error) { return nil, nil }

func JoinHostPort(host, port string) string { return host + ":" + port }
