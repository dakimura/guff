package bad

import "errors"

func bad() {
	var err error
	var p *error
	errors.As(err, p)
}
