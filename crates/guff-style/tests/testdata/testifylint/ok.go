package testifylint

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestOk(t *testing.T) {
	var result bool
	var err error
	var ptr *int
	arr := []int{1}
	str := "hi"
	f := 1.5

	assert.False(t, result)
	assert.True(t, result)
	assert.Equal(t, a, b)
	assert.Empty(t, arr)
	assert.Empty(t, str)
	assert.NoError(t, err)
	assert.Nil(t, ptr)
	assert.Len(t, arr, 3)
	assert.InEpsilon(t, 1.5, f, 0.0001)
}

var a, b int
