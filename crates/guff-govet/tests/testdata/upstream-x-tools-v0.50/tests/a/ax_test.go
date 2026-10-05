package a_test

import _ "example.com/tests/a"

func ExampleFoo() {} // OK because a.Foo exists

func ExampleBar() {} // want "ExampleBar refers to unknown identifier: Bar"
