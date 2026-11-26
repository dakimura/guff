package json

import "io"

func Marshal(v any) ([]byte, error)                              { return nil, nil }
func MarshalIndent(v any, prefix, indent string) ([]byte, error) { return nil, nil }

type Encoder struct{}

func NewEncoder(w io.Writer) *Encoder { return nil }

func (e *Encoder) Encode(v any) error { return nil }
