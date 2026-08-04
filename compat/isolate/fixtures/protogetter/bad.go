package p

type User struct{ Name string }

func (u *User) GetName() string { return u.Name }
func (u *User) ProtoReflect()   {}

func Bad(u *User) string {
	return u.Name
}
