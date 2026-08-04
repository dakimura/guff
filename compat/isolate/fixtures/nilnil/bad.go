package p

type User struct{}

func Bad() (*User, error) {
	return nil, nil
}
