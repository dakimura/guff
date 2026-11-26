package user

type User struct{ Uid string }

type Group struct{ Gid string }

func (u *User) GroupIds() ([]string, error) { return nil, nil }

func Lookup(username string) (*User, error) { return nil, nil }
func LookupId(id string) (*User, error)     { return nil, nil }
func Current() (*User, error)               { return nil, nil }
