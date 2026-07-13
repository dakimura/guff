package main
func f() { for { break; defer func(){}() } }
