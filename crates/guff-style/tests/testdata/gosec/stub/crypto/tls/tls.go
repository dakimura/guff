package tls

type Config struct{}

type Listener interface {
	Close() error
}

func Listen(network, laddr string, config *Config) (Listener, error) { return nil, nil }
