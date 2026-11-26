package json

func Marshal(v interface{}) (b []byte, err error) { return []byte{}, nilErr{} }

type nilErr struct{}

func (nilErr) Error() string { return "" }

func Unmarshal(data []byte, v interface{}) error { return nilErr{} }
