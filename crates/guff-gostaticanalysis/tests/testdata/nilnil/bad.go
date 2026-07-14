package nilnil

type User struct{}

func bad() (*User, error) {
	return nil, nil
}
