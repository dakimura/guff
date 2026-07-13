package main

import ("errors"; "fmt")

func f() error { return errors.New(fmt.Sprintf("x")) }
