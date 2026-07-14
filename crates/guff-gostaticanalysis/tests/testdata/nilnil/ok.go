package nilnil

type User struct{}

func ok() (*User, error) {
	return &User{}, nil
}
