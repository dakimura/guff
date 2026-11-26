package musttag

import "encoding/json"

type User struct {
	Name string
	Age  int
}

func marshalBad() {
	u := User{Name: "a", Age: 1}
	_, _ = json.Marshal(u)
}

func unmarshalBad() {
	var u User
	_ = json.Unmarshal([]byte(`{}`), &u)
}
