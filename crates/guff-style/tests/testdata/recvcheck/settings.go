package example

type JSON struct{}

func (j JSON) MarshalJSON() ([]byte, error) {
	return nil, nil
}

func (j *JSON) UnmarshalJSON(b []byte) error {
	return nil
}

type SQL struct{}

func (s SQL) Value() (any, error) {
	return nil, nil
}

func (s *SQL) Scan(src any) error {
	return nil
}
