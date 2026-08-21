package gosec_g123

// G123 (securego/gosec analyzers/tls_resumption_verifypeer.go) reports a
// `tls.Config` that installs a custom `VerifyPeerCertificate` but leaves TLS
// session resumption able to skip it: a resumed session presents no
// certificate chain, so the callback never runs. Setting `VerifyConnection`
// (which *does* run on a resumed session) or `SessionTicketsDisabled: true`
// closes the hole.
//
// Each config below is marked `// FINDING` or `// silent`, and
// compat/golden/cases/gosec runs golangci-lint 2.12.2 over this same file.
//
// The analyzer is an inventory of field *stores*, not a dataflow: it does not
// matter whether the config is a composite literal or assigned afterwards, and
// it does not matter whether the config is ever used.

import (
	"crypto/tls"
	"crypto/x509"
)

func verify(rawCerts [][]byte, chains [][]*x509.Certificate) error { return nil }

func direct() *tls.Config {
	return &tls.Config{
		VerifyPeerCertificate: verify, // FINDING
	}
}

func withVerifyConnection() *tls.Config {
	return &tls.Config{
		VerifyPeerCertificate: verify, // silent
		VerifyConnection:      func(cs tls.ConnectionState) error { return nil },
	}
}

func withTicketsDisabled() *tls.Config {
	return &tls.Config{
		VerifyPeerCertificate:  verify, // silent
		SessionTicketsDisabled: true,
	}
}

func assignedAfter() *tls.Config {
	c := &tls.Config{}
	c.VerifyPeerCertificate = verify // FINDING
	return c
}

func explicitNil() *tls.Config {
	return &tls.Config{
		VerifyPeerCertificate: nil, // silent: a nil store does not set the field
	}
}

// The config is built in another function, so its field record is keyed by a
// value this one never sees — the hand-off is not followed across the call.
func viaGetConfigForClientCall() *tls.Config {
	return &tls.Config{
		GetConfigForClient: func(*tls.ClientHelloInfo) (*tls.Config, error) { // silent
			return direct(), nil
		},
	}
}

// Built *inside* the closure, so both the inner config and the hand-off that
// exposes it are reported.
func viaGetConfigForClientInline() *tls.Config {
	return &tls.Config{
		GetConfigForClient: func(*tls.ClientHelloInfo) (*tls.Config, error) { // FINDING
			inner := &tls.Config{VerifyPeerCertificate: verify} // FINDING
			return inner, nil
		},
	}
}

// The root walk climbs through `FieldAddr`, so a config embedded in another
// struct is tracked the same way.
func fieldOfStruct() *tls.Config {
	type holder struct{ cfg tls.Config }
	h := &holder{}
	h.cfg.VerifyPeerCertificate = verify // FINDING
	return &h.cfg
}

func plain() *tls.Config {
	return &tls.Config{} // silent
}
