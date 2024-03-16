///usr/bin/env scriptisto "$0" "$@" ; exit $?

package main

// scriptisto-begin
// script_src: main.go
// deps: ["github.com/fatih/color v1.16.0"]
// build_cmd: go build -o script main.go
// scriptisto-end

import "github.com/fatih/color"

func main() {
	color.Yellow("Hello, Go!")
}
