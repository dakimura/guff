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

type SetupAllSuite interface {
	SetupSuite()
}

type SetupTestSuite interface {
	SetupTest()
}

type TearDownAllSuite interface {
	TearDownSuite()
}

type TearDownTestSuite interface {
	TearDownTest()
}

type BeforeTest interface {
	BeforeTest(suiteName, testName string)
}

type AfterTest interface {
	AfterTest(suiteName, testName string)
}

type SuiteInformation struct{}

type WithStats interface {
	HandleStats(suiteName string, stats *SuiteInformation)
}

type SetupSubTest interface {
	SetupSubTest()
}

type TearDownSubTest interface {
	TearDownSubTest()
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
