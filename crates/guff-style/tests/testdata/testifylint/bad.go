package testifylint

import (
	"errors"
	"fmt"
	"regexp"
	"strings"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/suite"
)

func TestBad(t *testing.T) {
	var result bool
	var err error
	var ptr *int
	arr := []int{1}
	str := "hi"
	f := 1.5
	ts := time.Time{}
	t1 := time.Time{}
	t2 := time.Time{}
	signed := -1
	pos := 1
	errSentinel := errors.New("sentinel")
	body := `{"x":1}`
	expectedJSON := `{}`
	expectedYML := "k: v"
	conf := "k: v"
	expected := 42

	assert.Equal(t, false, result)
	assert.Equal(t, true, result)
	assert.True(t, result == true)
	assert.True(t, a == b)
	assert.Equal(t, 0, len(arr))
	assert.Equal(t, "", str)
	assert.Nil(t, err)
	assert.Equal(t, nil, err)
	assert.Equal(t, nil, ptr)
	assert.Equal(t, 3, len(arr))
	assert.Equal(t, 1.5, f)

	assert.Equal(t, time.Time{}, ts)
	assert.True(t, ts.IsZero())
	assert.Equal(t, t1, t2)
	assert.Less(t, signed, 0)
	assert.Greater(t, pos, 0)
	assert.Equal(t, signed, signed)
	assert.Zero(t, 42)
	assert.True(t, true)

	assert.True(t, strings.Contains(str, "hi"))
	assert.Contains(t, arr, 1, 2)
	assert.EqualValues(t, 42, pos)
	assert.Regexp(t, regexp.MustCompile(`hi`), str)

	assert.Error(t, err, errSentinel)
	assert.True(t, errors.Is(err, errSentinel))
	assert.IsType(t, err, errSentinel)

	assert.Equal(t, `{"foo":"bar"}`, body)
	assert.Equal(t, expectedJSON, body)
	assert.Equal(t, expectedYML, conf)

	assert.Equal(t, pos, expected)
	assert.Equal(t, pos, 42)

	assert.Equal(t, 1, 2, fmt.Sprintf("msg"))
	assert.Equal(t, 1, 2, 42)
	assert.Fail(t, "case [%d] failed", 1)
}

type SuiteBad struct {
	suite.Suite
}

func (s *SuiteBad) TestSuiteAntiPatterns() {
	var result any
	b := true

	s.Assert().True(b)
	assert.Equal(s.T(), 42, result)
	s.T().Run("sub", func(t *testing.T) {
		assert.Equal(t, 1, 2)
	})
}

func (s *SuiteBad) TestWithArgs(t *testing.T) {
	s.True(true)
}

func (s *SuiteBad) SetupTest(_ int) {}

func (s *SuiteBad) TestParallelBroken() {
	s.T().Parallel()
	s.Run("sub", func() {
		s.T().Parallel()
	})
}

func (s *SuiteBad) helperWithoutTHelper() {
	s.Equal(1, 2)
}

var a, b int
