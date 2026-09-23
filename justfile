set dotenv-load

# The correct implementation (no API calls, no tokens, no fun)
default: fizzbuzz

fizzbuzz:
    cargo run -q -- fizzbuzz

# List the strategies and the question each one asks
list:
    cargo run -q -- list

# Run one strategy, e.g. `just run vibes --to 30 --verbose` or `just run print --data words`
run strategy="divisible" *args:
    cargo run -q -- run {{strategy}} {{args}}

# Evaluate every strategy over 1..=100 and write results/latest.{json,md,html}
eval *args:
    cargo run -q --release -- eval {{args}}

# Same, under every data form: number, words, binary
eval-all *args:
    cargo run -q --release -- eval --data number,words,binary {{args}}

# Rebuild results/latest.html from results/latest.json (no API calls)
report *args:
    cargo run -q -- report {{args}}

# Open the HTML report
open:
    open results/latest.html

test:
    cargo test -q

check:
    cargo fmt --check && cargo clippy -q -- -D warnings

# One raw request, to confirm the key works
ping:
    curl -sS -X POST https://api.typesafe.ai/v1/systemone \
      -H "Authorization: Bearer $TYPESAFE_API_KEY" \
      -H "Content-Type: application/json" \
      -d '{"state":"15","model":"jev-latest","questions":{"fizz":{"type":"noul","instructions":"Is this fizz?"}}}'
