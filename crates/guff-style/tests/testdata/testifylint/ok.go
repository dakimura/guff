package testifylint

import (
	"errors"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
	"github.com/stretchr/testify/suite"
)

func TestOk(t *testing.T) {
	var result bool
	var err error
	var ptr *int
	arr := []int{1}
	str := "hi"
	f := 1.5
	var ts time.Time
	var t1, t2 time.Time
	signed := -1
	pos := 1
	var anyVal any = 42
	errSentinel := errors.New("sentinel")
	body := `{"foo":"bar"}`
	expectedYML := "k: v"
	conf := "k: v"
	expected := 42

	assert.False(t, result)
	assert.True(t, result)
	assert.Equal(t, a, b)
	assert.Empty(t, arr)
	assert.Empty(t, str)
	require.NoError(t, err)
	assert.Nil(t, ptr)
	assert.Len(t, arr, 3)
	assert.InEpsilon(t, 1.5, f, 0.0001)
	assert.Zero(t, ts)
	assert.Negative(t, signed)
	assert.Positive(t, pos)
	assert.Contains(t, str, "hi")
	assert.Equal(t, 42, pos)
	assert.EqualValues(t, 42, anyVal)
	assert.Regexp(t, `hi`, str)

	require.ErrorIs(t, err, errSentinel)
	assert.JSONEq(t, `{"foo":"bar"}`, body)
	assert.YAMLEq(t, expectedYML, conf)
	assert.Equal(t, expected, pos)

	assert.True(t, t1.Equal(t2))
	assert.Equal(t, t1.UTC(), t2.UTC())
	assert.Equal(t, 1, 2, "msg")
	assert.Equalf(t, 1, 2, "msg %d", 42)
	assert.Fail(t, "boom!", "case [%d] failed", 1)
}

type SuiteOk struct {
	suite.Suite
}

func (s *SuiteOk) SetupTest() {}

func (s *SuiteOk) TestSuiteIdiomatic() {
	var result any
	b := true

	s.True(b)
	s.Equal(42, result)
	s.Run("sub", func() {
		s.Equal(1, 2)
	})
}

func (s *SuiteOk) helperWithTHelper() {
	s.T().Helper()
	s.Equal(1, 2)
}

var a, b int
