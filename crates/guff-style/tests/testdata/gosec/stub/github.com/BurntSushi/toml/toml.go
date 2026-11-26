package toml

import "io"

type Encoder struct{}

func NewEncoder(w io.Writer) *Encoder { return nil }

func (e *Encoder) Encode(v any) error { return nil }
