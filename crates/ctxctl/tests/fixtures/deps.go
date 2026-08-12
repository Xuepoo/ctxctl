// Package deps exercises the deps command (go).
package deps

import (
	"fmt"
	_ "embed"
	"github.com/x/y"
	"localpkg/helper"
)

func main() {
	fmt.Println(helper.Help())
}
