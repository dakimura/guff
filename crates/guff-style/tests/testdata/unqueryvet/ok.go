package unqueryvetok

func ok() {
	_ = "SELECT id, name FROM users"
	_ = "SELECT COUNT(*) FROM users"
	_ = "SELECT * FROM information_schema.tables"
	_ = "SELECT * FROM pg_catalog.pg_tables"
	dbQuery("SELECT MAX(*) FROM x")
}

func dbQuery(q string) {}
