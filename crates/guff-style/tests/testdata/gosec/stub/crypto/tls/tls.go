package tls

type Config struct {
	InsecureSkipVerify bool
	MinVersion         uint16
	MaxVersion         uint16
}

type Listener interface {
	Close() error
}

func Listen(network, laddr string, config *Config) (Listener, error) { return nil, nil }
