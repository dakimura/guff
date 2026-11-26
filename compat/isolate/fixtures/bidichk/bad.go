package p

// bidichk names the sequence it found, and there are eight names. One string
// with one override reaches one of them.

func Overrides() {
	_ = "Hello‮World" // RIGHT-TO-LEFT-OVERRIDE
	_ = "Hello‭World" // LEFT-TO-RIGHT-OVERRIDE
	_ = "Hello‪World" // LEFT-TO-RIGHT-EMBEDDING
	_ = "Hello‫World" // RIGHT-TO-LEFT-EMBEDDING
	_ = "Hello‬World" // POP-DIRECTIONAL-FORMATTING
	_ = "Hello⁦World" // LEFT-TO-RIGHT-ISOLATE
	_ = "Hello⁧World" // RIGHT-TO-LEFT-ISOLATE
	_ = "Hello⁨World" // FIRST-STRONG-ISOLATE
	_ = "Hello⁩World" // POP-DIRECTIONAL-ISOLATE
}
