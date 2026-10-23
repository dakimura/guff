package diff

import (
	"math/rand"
	"time"
)

type EditType uint8

const (
	Identity EditType = iota
	UniqueX
	UniqueY
	Modified
)

type EditScript []EditType

func (es EditScript) String() string {
	panic("stub")
}
func (es EditScript) stats() (s struct{ NI, NX, NY, NM int }) {
	panic("stub")
}
func (es EditScript) Dist() int {
	panic("stub")
}
func (es EditScript) LenX() int {
	panic("stub")
}
func (es EditScript) LenY() int {
	panic("stub")
}

type EqualFunc func(ix int, iy int) Result
type Result struct{ NumSame, NumDiff int }

func BoolResult(b bool) Result {
	panic("stub")
}
func (r Result) Equal() bool {
	panic("stub")
}
func (r Result) Similar() bool {
	panic("stub")
}

var randBool = rand.New(rand.NewSource(time.Now().Unix())).Intn(2) == 0

func Difference(nx, ny int, f EqualFunc) (es EditScript) {
	panic("stub")
}

type path struct {
	dir int
	point
	es EditScript
}

func (p *path) connect(dst point, f EqualFunc) {
	panic("stub")
}
func (p *path) append(t EditType) {
	panic("stub")
}

type point struct{ X, Y int }

func (p *point) add(dx, dy int) {
	panic("stub")
}
func zigzag(x int) int {
	panic("stub")
}
