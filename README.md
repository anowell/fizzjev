# fizzjev

FizzBuzz, but every decision is outsourced to a calibrated decision model.

> **Interviewer:** Print the numbers from 1 to 100, except that if the number is divisible by 3 print "fizz", if it's divisible by 5 print "buzz", and if it's divisible by 15 print "fizzbuzz".
>
> **Candidate:** Sure. First I'll need an API key.

In 2016, Joel Grus [solved FizzBuzz with TensorFlow](https://joelgrus.com/2016/05/23/fizz-buzz-in-tensorflow/). A decade later we have [Jev](https://docs.typesafe.ai) whose [known-jagged-edges page](https://docs.typesafe.ai/model-jaggedness/jev-1.13)
says: *"Jev is not a calculator... implement any mathematical logic in code."*
This repo is an eval of how many different ways you can ignore that advice, written in
Rust because the task was not yet absurd enough.

## Running it

Install rust and [just](https://just.systems/)

```sh
echo 'TYPESAFE_API_KEY=...' > .env
just fizzbuzz          # the correct implementation. 0 tokens.
just list              # the strategies and the question each one asks
just eval              # every strategy over 1..=100 -> results/latest.{json,md,html}
just eval-all          # same, under every data form (number, words, binary)
just report            # rebuild results/latest.html from results/latest.json, no API calls
just open              # open the HTML report
```

## What varies

Two axes are varied independently.

**Strategy**: which questions are asked and how the answers are combined.

| name              | asks      | question                                                                            |
|-------------------|-----------|-------------------------------------------------------------------------------------|
| `always-number`   | none      | Always print the number                                                             |
| `random`          | none      | Pick one of {number, Fizz, Buzz, FizzBuzz} uniformly at random                      |
| `random-weighted` | none      | Pick at random with the true odds: 8/15 number, 4/15 Fizz, 2/15 Buzz, 1/15 FizzBuzz |
| `vibes`           | y/n ×3    | Is this {fizz, buzz, fizzbuzz}?                                                     |
| `vibes-game`      | y/n ×3    | In FizzBuzz, is this number a {Fizz, Buzz, FizzBuzz}?                               |
| `vibes-rules`     | y/n ×3    | Is this {fizz, buzz, fizzbuzz}? (+ the divisibility rule as true/false criteria)    |
| `divisible`       | y/n ×2    | Is the number divisible by {3, 5}?                                                  |
| `print`           | choice    | What does FizzBuzz print for the number? {number, Fizz, Buzz, FizzBuzz}             |
| `classic`         | choice    | Given the interviewer's spec, what goes on the whiteboard? {number, Fizz, Buzz, FizzBuzz} |
| `modulo`          | choice ×2 | What is the remainder when the number is divided by {3, 5}? {0, 1, 2} / {0..4}      |
| `fizziness`       | score     | How fizzy is the number? Flat < Fizzy < Fizzy & Buzzy < Buzzy                    |

"asks" is the Jev question type: `y/n` is a noul (returns a probability of yes),
`choice` picks one option and returns a distribution, `score` rates against ordered
levels.

**Data form**: how the number is written into the state, for any strategy.

| form     | state                                                   |
|----------|---------------------------------------------------------|
| `number` | `{"number": 42}`                                        |
| `words`  | `{"number": "forty-two", "number_encoding": "written out in English words"}` |
| `binary` | `{"number": [0,1,0,1,0,1,0,0,0,0], "number_encoding": "10 binary digits, least significant digit first"}` |

