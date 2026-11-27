package usedandfmtadd

//go:embed data.txt
var s string

func F() { fmt.Println(s) }
