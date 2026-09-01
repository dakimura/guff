// Package g402 is gosec's G402 cipher-suite half.
//
// `CipherSuites: []uint16{…}` is read by the *name* of each selector, against
// the `goodCiphers` table of `NewIntermediateTLSCheck` — the constructor
// `rulelist.go` gives G402. Nothing is resolved: an element that is not a
// selector is skipped rather than reported.
//
// `Match` returns the **first** issue a `tls.Config` literal produces and
// stops, so a config with two problems is one finding.
package g402

import "crypto/tls"

// fires — the first cipher that is not on the list, named in the message.
func BadCipher() *tls.Config {
	return &tls.Config{
		CipherSuites: []uint16{
			tls.TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
			tls.TLS_ECDHE_RSA_WITH_AES_128_CBC_SHA,
			tls.TLS_RSA_WITH_3DES_EDE_CBC_SHA,
		},
	}
}

// silent — every cipher is on the list.
func GoodCiphers() *tls.Config {
	return &tls.Config{
		CipherSuites: []uint16{
			tls.TLS_AES_128_GCM_SHA256,
			tls.TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
			tls.TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
		},
	}
}

// fires once — and it is the `InsecureSkipVerify`, because that field comes
// first in the literal.
func BothBad() *tls.Config {
	return &tls.Config{
		InsecureSkipVerify: true,
		CipherSuites: []uint16{
			tls.TLS_RSA_WITH_AES_128_CBC_SHA,
		},
	}
}

// fires once — the same two fields the other way round, and now it is the
// cipher.
func BothBadReversed() *tls.Config {
	return &tls.Config{
		CipherSuites: []uint16{
			tls.TLS_RSA_WITH_AES_128_CBC_SHA,
		},
		InsecureSkipVerify: true,
	}
}

// silent — an empty list.
func EmptyCiphers() *tls.Config {
	return &tls.Config{CipherSuites: []uint16{}}
}

// silent — a variable rather than a literal list.
var suites = []uint16{tls.TLS_RSA_WITH_AES_128_CBC_SHA}

func VarCiphers() *tls.Config {
	return &tls.Config{CipherSuites: suites}
}

// silent — an assignment through a `*tls.Config`. `Match`'s assign branch wants
// the receiver's type to be `crypto/tls.Config` exactly, and a pointer is not.
func AssignCipher(cfg *tls.Config) {
	cfg.CipherSuites = []uint16{tls.TLS_RSA_WITH_AES_128_CBC_SHA}
}

// silent — a plain constant in the list is not a selector.
const custom uint16 = 0x1301

func ConstCipher() *tls.Config {
	return &tls.Config{CipherSuites: []uint16{custom}}
}
