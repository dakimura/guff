package testifylint

import (
	"regexp"
	"strings"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
)

func TestBad(t *testing.T) {
	var result bool
	var err error
	var ptr *int
	arr := []int{1}
	str := "hi"
	f := 1.5
	ts := time.Time{}
	signed := -1
	pos := 1

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
	assert.Less(t, signed, 0)
	assert.Greater(t, pos, 0)
	assert.Equal(t, signed, signed)
	assert.Zero(t, 42)
	assert.True(t, true)

	assert.True(t, strings.Contains(str, "hi"))
	assert.Contains(t, arr, 1, 2)
	assert.EqualValues(t, 42, pos)
	assert.Regexp(t, regexp.MustCompile(`hi`), str)
}

var a, b int
