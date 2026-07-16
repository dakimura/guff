package testifylint

import (
	"errors"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
)

func TestOk(t *testing.T) {
	var result bool
	var err error
	var ptr *int
	arr := []int{1}
	str := "hi"
	f := 1.5
	var ts time.Time
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
	assert.NoError(t, err)
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

	assert.ErrorIs(t, err, errSentinel)
	assert.JSONEq(t, `{"foo":"bar"}`, body)
	assert.YAMLEq(t, expectedYML, conf)
	assert.Equal(t, expected, pos)
}

var a, b int
