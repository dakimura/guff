package pkg

func ok1() (int, error) { return 0, nil }
func ok2() error        { return nil }
func ok3() (error, bool) { return nil, false }
func ok4() (int, error, bool) { return 0, nil, false }
func ok5() (a int, b error) { return 0, nil }
