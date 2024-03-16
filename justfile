build:
  cargo build

release:
  cargo build --release
  ls -al target/release/scriptisto
  cp target/release/scriptisto ~/bin/

run-go:
  cargo run --bin scriptisto -- temp/main.go
