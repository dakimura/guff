package x509

// Enough of crypto/x509 for the G123 fixture: the certificate type only ever
// appears in `VerifyPeerCertificate`'s signature.

type Certificate struct{}

func ParseCertificate(der []byte) (*Certificate, error) { return nil, nil }
