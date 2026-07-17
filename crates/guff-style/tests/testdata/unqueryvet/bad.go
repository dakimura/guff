package unqueryvet

func bad() {
	query := "SELECT * FROM users"
	_ = query
	dbQuery("SELECT * FROM products")
	const q = `SELECT *
FROM orders`
	_ = q
	_ = "SELECT t.* FROM users t"
}

func dbQuery(q string) {}
