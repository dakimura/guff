// Package user consumes the facts: unparam stops counting what follows a call
// that cannot return, and SA4017 reports a discarded call to a pure function.
package user

import (
	"example.com/dep"
	"example.com/mid"
)

func afterDie(p bool) {
	dep.Die()
	if p {
		println(1)
	}
}

func afterMethodDie(p bool) {
	dep.T{}.Die()
	if p {
		println(1)
	}
}

func afterChain(p bool) {
	mid.Exit()
	if p {
		println(1)
	}
}

func afterFail(p bool) {
	dep.Fail(true)
	if p {
		println(1)
	}
}

func Callers() {
	afterDie(true)
	afterDie(false)
	afterMethodDie(true)
	afterMethodDie(false)
	afterChain(true)
	afterChain(false)
	afterFail(true)
	afterFail(false)
}

func local(a int) int { return dep.Add(a, a) }

func Discards() {
	dep.Add(1, 2)
	dep.Twice(3)
	dep.P{}.Sum()
	mid.Thrice(4)
	local(5)

	dep.Print(6)
	dep.Zero(7)
	mid.Loud(8)
}
