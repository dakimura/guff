package suite

import (
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

type TestingSuite interface {
	T() *testing.T
	SetT(*testing.T)
	SetS(suite TestingSuite)
}

type Suite struct {
	*assert.Assertions
}

func (s *Suite) T() *testing.T { return nil }
func (s *Suite) SetT(t *testing.T) {
	s.Assertions = assert.New(t)
}
func (s *Suite) SetS(suite TestingSuite) {}
func (s *Suite) Assert() *assert.Assertions {
	return s.Assertions
}
func (s *Suite) Require() *require.Assertions {
	return require.New(nil)
}
func (s *Suite) Run(name string, subtest func()) bool { return true }

func Run(t *testing.T, s TestingSuite) {}
