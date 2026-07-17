package gochecksumtype

import "log"

//sumtype:decl
type SumType interface{ isSumType() }

//sumtype:decl
type One struct{} // not an interface

func (One) isSumType() {}

type Two struct{}

func (Two) isSumType() {}

func sumTypeTest() {
	var sum SumType = One{}
	switch sum.(type) {
	case One:
	}

	switch sum.(type) {
	case One:
	default:
		panic("??")
	}

	switch sum.(type) {
	case *One:
	default:
		log.Println("legit catch all goes here")
	}

	log.Println("??")

	switch sum.(type) {
	case One:
	case Two:
	}
}
