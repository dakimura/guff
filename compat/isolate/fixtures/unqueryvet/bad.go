package p

func Bad() {
	_ = "SELECT * FROM users"
}

// unqueryvet looks at more than a bare string: the same star-select inside a
// call argument, a constant, and a raw literal are separate sites.
const query = "SELECT * FROM accounts"

func Call() {
	sink("SELECT * FROM orders")
}

func sink(string) {}

func Raw() {
	_ = `SELECT * FROM logs`
}
