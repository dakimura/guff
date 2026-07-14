package json

import "io"

type Encoder struct{}

func NewEncoder(w io.Writer) *Encoder {
	return &Encoder{}
}

func (e *Encoder) Encode(v any) error {
	return nil
}

func Marshal(v any) ([]byte, error) {
	return nil, nil
}

func MarshalIndent(v any, prefix, indent string) ([]byte, error) {
	return nil, nil
}
