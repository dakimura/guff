package pb

// User is a stand-in for a protoc-generated message: it carries exported
// fields plus generated getters and the v2 ProtoReflect marker.
type User struct {
	Name    string
	Age     int32
	Address *Address
	Meta    map[string]string
	Names   []string
	// An `optional` proto3 field: the struct holds a pointer and the getter
	// hands back the value.
	Nickname *string
}

type Address struct {
	City string
}

func (x *User) GetName() string {
	if x != nil {
		return x.Name
	}
	return ""
}

func (x *User) GetAge() int32 {
	if x != nil {
		return x.Age
	}
	return 0
}

func (x *User) GetAddress() *Address {
	if x != nil {
		return x.Address
	}
	return nil
}

func (x *User) GetMeta() map[string]string {
	if x != nil {
		return x.Meta
	}
	return nil
}

func (x *User) GetNames() []string {
	if x != nil {
		return x.Names
	}
	return nil
}

func (x *User) GetNickname() string {
	if x != nil && x.Nickname != nil {
		return *x.Nickname
	}
	return ""
}

func (x *User) ProtoReflect() {}

func (x *Address) GetCity() string {
	if x != nil {
		return x.City
	}
	return ""
}

func (x *Address) ProtoReflect() {}
