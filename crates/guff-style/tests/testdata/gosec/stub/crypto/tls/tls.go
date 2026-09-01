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
	CipherSuites       []uint16

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

// The cipher-suite constants G402 names. Their *values* are irrelevant: the
// rule matches the selector's name against its table.
const (
	TLS_AES_128_GCM_SHA256                  uint16 = 0x1301
	TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256   uint16 = 0xc02f
	TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256 uint16 = 0xc02b
	TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384   uint16 = 0xc030
	TLS_ECDHE_RSA_WITH_AES_128_CBC_SHA      uint16 = 0xc013
	TLS_RSA_WITH_AES_128_CBC_SHA            uint16 = 0x002f
	TLS_RSA_WITH_3DES_EDE_CBC_SHA           uint16 = 0x000a
)
