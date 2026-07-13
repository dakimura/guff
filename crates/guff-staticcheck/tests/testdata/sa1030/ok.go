package main

import "strconv"

func main() {
	strconv.ParseInt("1", 10, 64)
	strconv.ParseFloat("1", 64)
	strconv.FormatInt(1, 10)
	strconv.FormatFloat(1, 'f', 0, 64)
}
