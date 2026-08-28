package fixture

func describe(v interface{}) interface{} { return v }

var boxes []interface{}

func tail(s []byte) []byte { return s[1:len(s)] }
