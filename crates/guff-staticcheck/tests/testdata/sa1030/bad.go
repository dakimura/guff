package main

import "strconv"

func main() {
	strconv.ParseInt("1", 1, 0)
	strconv.ParseInt("1", 0, 65)
	strconv.ParseFloat("1", 16)
	strconv.FormatInt(1, 1)
	strconv.FormatFloat(1, 'z', 0, 16)
}
