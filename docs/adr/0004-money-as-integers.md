# Money is an integer number of nano-units, never a float

A `Money` type holds a whole number of nano-units (one billionth) of a currency in an `i64`, with the currency beside it. Prices per million tokens, message cost, plans, estimates and bills are all `Money`. Float arithmetic is forbidden by lint. Rounding to the currency's minor unit happens only when a figure is shown or exported. Fractions of a split and shares of a plan are divided with the largest-remainder method, so the parts always add up to the whole.

## Why

Providers price per million tokens, so one token costs a fraction of a cent and whole cents lose it. Floats drift, and the reconciliation must come out to the cent. An `i64` of nano-units holds over nine billion units of currency, enough for any organization. `rust_decimal` is the fallback if a fixed unit turns out too limiting.
