package sub

import "errors"

type Res struct{}

func E() error { return errors.New("e") }
