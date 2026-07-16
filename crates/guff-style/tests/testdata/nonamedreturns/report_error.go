package nonamedreturnsreport

// With report-error-in-defer, the defer/error exemption is disabled.
func errorInDeferAssigned() (err error) { // want
	defer func() {
		err = nil
	}()
	return
}

func namedInt() (i int) { // want
	return 0
}
