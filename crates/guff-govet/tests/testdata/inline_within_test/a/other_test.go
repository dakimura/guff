package a

import "testing"

func Test(t *testing.T) {
	print(K) // inlined
	var _ A  // inlined
}

func TestK(t *testing.T) {
	print(K) // not inlined: TestK is K's test
	var _ A  // inlined
}

func TestA_comment(t *testing.T) {
	var _ A // not inlined: TestA_comment is A's test
}

func TestKey(t *testing.T) {
	print(K) // inlined: TestKey is not TestK
}

func ExampleK() {
	print(K) // not inlined
}

type helper struct{}

func (helper) TestK() {
	print(K) // inlined: a method is not a test
}
