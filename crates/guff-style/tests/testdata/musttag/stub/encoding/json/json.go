package json

type Encoder struct{}

type Decoder struct{}

func NewEncoder() *Encoder { return &Encoder{} }

func NewDecoder() *Decoder { return &Decoder{} }

func (e *Encoder) Encode(v any) error { return nil }

func (d *Decoder) Decode(v any) error { return nil }

func Marshal(v any) ([]byte, error) { return nil, nil }

func MarshalIndent(v any, prefix, indent string) ([]byte, error) {
	return nil, nil
}

func Unmarshal(data []byte, v any) error { return nil }
