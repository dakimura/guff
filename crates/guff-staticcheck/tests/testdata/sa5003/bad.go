package main
func f() { for { defer func(){}() } }
