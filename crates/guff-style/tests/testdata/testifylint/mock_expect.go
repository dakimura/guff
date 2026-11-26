package mockexpect

import (
	"testing"

	"github.com/stretchr/testify/mock"
)

type MockUser struct {
	mock.Mock
}

type MockUser_Expecter struct {
	mock *mock.Mock
}

func (_m *MockUser) EXPECT() *MockUser_Expecter {
	return &MockUser_Expecter{mock: &_m.Mock}
}

func (_m *MockUser) Void() {
	_m.Called()
}

func (_e *MockUser_Expecter) Void() *mock.Call {
	return _e.mock.On("Void")
}

func (_m *MockUser) CountUsers() int {
	_m.Called()
	return 0
}

func (_e *MockUser_Expecter) CountUsers() *mock.Call {
	return _e.mock.On("CountUsers")
}

func (_e *MockUser_Expecter) Variadic(values ...interface{}) *mock.Call {
	return _e.mock.On("Variadic", values...)
}

func (_e *MockUser_Expecter) CreateUser(_a0 interface{}, _a1 interface{}) *mock.Call {
	return _e.mock.On("CreateUser", _a0, _a1)
}

type mockHolder struct {
	user *MockUser
}

func mockFrom(m *MockUser) *MockUser { return m }

const voidMethod = "Void"

type User struct {
	Name string
}

type otherExpecter struct{}

func (*otherExpecter) Void() {}

type otherMock struct{}

func (*otherMock) On(string, ...interface{}) {}
func (*otherMock) EXPECT() *otherExpecter    { return &otherExpecter{} }

func TestMockExpect(t *testing.T) {
	u := &MockUser{}
	holder := mockHolder{user: u}
	values := []interface{}{1, 2, 3}

	// Invalid.
	u.On("CreateUser", mock.Anything, User{}).Return(nil)
	u.On("Void")
	u.On("Void").Once()
	u.On("CountUsers").Return(123)
	u.On("Variadic", values...)
	u.On("Variadic", 1, 2, 3)
	u.On("Variadic")
	holder.user.On("Void")
	mockFrom(u).On("Void")
	u.On(voidMethod)
	u.On("Void").Run(func(mock.Arguments) {})
	u.On("Void").Once().Run(func(mock.Arguments) {}).Twice()

	// Valid.
	u.EXPECT().CreateUser(mock.Anything, User{}).Return(nil)
	u.EXPECT().Void()
	u.EXPECT().CountUsers().Return(123)
	u.EXPECT().Variadic(values...)
	u.EXPECT().Variadic(1, 2, 3)
	u.EXPECT().Variadic()
	holder.user.EXPECT().Void()
	mockFrom(u).EXPECT().Void()

	// Ignored.
	u.On("", mock.Anything, User{}).Return(nil)
	u.On("DoesNotExist", mock.Anything, User{}, 123).Return(nil)
	u.On("Void", 123)
	u.On("CreateUser", mock.Anything)
	u.On("Void", values...)
	other := &otherMock{}
	other.On("Void")
}
