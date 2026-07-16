package musttag

import "encoding/json"

type User struct {
	Name string `json:"name"`
	Age  int    `json:"age"`
}

type ignored struct {
	Secret string `json:"-"`
	Name   string `json:"name"`
}

type nested struct {
	User User `json:"user"`
}

func marshalOk() {
	u := User{Name: "a", Age: 1}
	_, _ = json.Marshal(u)
	_, _ = json.Marshal(ignored{Name: "x"})
	_, _ = json.Marshal(nested{User: u})
	_, _ = json.Marshal(nil)
}

func unmarshalOk() {
	var u User
	_ = json.Unmarshal([]byte(`{}`), &u)
}
