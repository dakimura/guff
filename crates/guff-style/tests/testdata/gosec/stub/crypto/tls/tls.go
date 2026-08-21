package tls

import "crypto/x509"

type ConnectionState struct{}
type ClientHelloInfo struct{}

type ClientSessionCache interface {
	Get(sessionKey string) (*ClientSessionState, bool)
}

type ClientSessionState struct{}

type Config struct {
	InsecureSkipVerify bool
	MinVersion         uint16
	MaxVersion         uint16

	// The G123 fields. Their *names* are what the analyzer keys on (it reads
	// the struct field at the FieldAddr's index), so they have to match the
	// standard library exactly.
	SessionTicketsDisabled bool
	ClientSessionCache     ClientSessionCache
	VerifyPeerCertificate  func(rawCerts [][]byte, verifiedChains [][]*x509.Certificate) error
	VerifyConnection       func(ConnectionState) error
	GetConfigForClient     func(*ClientHelloInfo) (*Config, error)
}

type Listener interface {
	Close() error
}

func Listen(network, laddr string, config *Config) (Listener, error) { return nil, nil }
