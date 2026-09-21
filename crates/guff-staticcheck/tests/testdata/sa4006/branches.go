// A redefinition inside an `if` arm is not normally an overwrite: the branch
// may not run, and the value assigned before the `if` is still live on the
// other path. But when every *other* arm leaves the function, the redefining
// arm is the only way out and the first value is dead after all.
//
// beats writes that twice (`x-pack/metricbeat/module/aws/mtest` and
// `.../gcp/metrics`):
//
//	config := map[string]interface{}{}
//	if !okAccessKeyID || accessKeyID == "" {
//	    t.Fatal("$AWS_ACCESS_KEY_ID not set or set to empty")
//	} else if !okSecretAccessKey || secretAccessKey == "" {
//	    t.Fatal("$AWS_SECRET_ACCESS_KEY not set or set to empty")
//	} else {
//	    config = map[string]interface{}{…}
//	}
//	config["default_region"] = defaultRegion
//
// `fatal` stands in for `t.Fatal` so the fixture needs no imports; `ctrlflow`
// proves both the same way.
package main

func fatal(msg string) { panic(msg) }

func warn(msg string) {}

func use(v map[string]any) {}

// fires — the beats shape: two leaving arms, one redefining `else`, and a read
// after the `if`.
func twoFatalArms(a, b string) map[string]any {
	config := map[string]any{}
	if a == "" {
		fatal("a")
	} else if b == "" {
		fatal("b")
	} else {
		config = map[string]any{"a": a}
	}
	config["x"] = 1
	return config
}

// fires — one arm, and it returns.
func returningArm(a string) map[string]any {
	config := map[string]any{}
	if a == "" {
		return nil
	} else {
		config = map[string]any{"a": a}
	}
	config["x"] = 1
	return config
}

// fires — the redefining arm is the first one and the others leave.
func redefiningArmFirst(a, b string) map[string]any {
	config := map[string]any{}
	if a != "" {
		config = map[string]any{"a": a}
	} else if b == "" {
		fatal("b")
	} else {
		panic("no")
	}
	config["x"] = 1
	return config
}

// silent — no `else`, so the fall-through arm redefines nothing.
func noElse(a string) map[string]any {
	config := map[string]any{}
	if a == "" {
		config = map[string]any{"a": a}
	}
	config["x"] = 1
	return config
}

// silent — the other arm reads the value instead of leaving.
func otherArmReads(a string) map[string]any {
	config := map[string]any{}
	if a == "" {
		config = map[string]any{"a": a}
	} else {
		use(config)
	}
	config["x"] = 1
	return config
}

// silent — `warn` returns, so the first arm falls through to the read.
func otherArmReturns(a string) map[string]any {
	config := map[string]any{}
	if a == "" {
		warn("a")
	} else {
		config = map[string]any{"a": a}
	}
	config["x"] = 1
	return config
}

// silent — `break` leaves the loop but stays in the function.
func breakIsNotLeaving(xs []string) map[string]any {
	config := map[string]any{}
	for _, x := range xs {
		if x == "" {
			break
		} else {
			config = map[string]any{"x": x}
		}
		config["y"] = 1
	}
	return config
}

// silent — the redefinition is one level deeper than the chain, so the arm
// running does not mean the redefinition ran.
func redefOneLevelDeeper(a, b string) map[string]any {
	config := map[string]any{}
	if a == "" {
		fatal("a")
	} else {
		if b != "" {
			config = map[string]any{"a": a}
		}
	}
	config["x"] = 1
	return config
}

// silent — the `if` is not in the assignment's own statement list, so the read
// is reachable without running it at all.
func chainNotInAssignsList(a, b string) map[string]any {
	config := map[string]any{}
	if a != "" {
		if b == "" {
			fatal("b")
		} else {
			config = map[string]any{"a": a}
		}
	}
	config["x"] = 1
	return config
}

// silent — the middle arm falls through.
func middleArmFallsThrough(a, b string) map[string]any {
	config := map[string]any{}
	if a == "" {
		fatal("a")
	} else if b == "" {
		use(config)
	} else {
		config = map[string]any{"a": a}
	}
	config["x"] = 1
	return config
}
