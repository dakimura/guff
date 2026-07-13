package bad

import "errors"

type netDNSError struct{}

func (e netDNSError) Error() string { return "dns" }

func bad() {
	var err error
	var target netDNSError
	errors.As(err, target)
}
