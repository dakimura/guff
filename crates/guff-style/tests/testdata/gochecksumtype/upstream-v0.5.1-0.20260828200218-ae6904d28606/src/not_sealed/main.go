package not_sealed

//sumtype:decl
type T interface{} // want `interface 'T' is not sealed`

func main() {}
