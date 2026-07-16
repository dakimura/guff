package testifylint

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestBad(t *testing.T) {
	var result bool
	var err error
	var ptr *int
	arr := []int{1}
	str := "hi"
	f := 1.5

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
}

var a, b int
