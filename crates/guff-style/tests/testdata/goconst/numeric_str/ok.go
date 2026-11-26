package numericstr

func ports() {
	// Default number_min=number_max=3 filters ParseInt-able strings even
	// when numbers/ParseNumbers is false (golangci + upstream ProcessResults).
	_ = "443"
	_ = "443"
	_ = "443"
	_ = "443"
}
