# Guest Programs

Each file in the [`src/bin`](./src/bin) folder defines a program for the zkVM.
We refer to the program running in the zkVM as the "[guest]".

To learn more about writing guest programs, check out the zkVM [developer docs].
For zkVM API documentation, see the [guest module] of the [`risc0-zkvm`] crate.

For the Malda protocol, we have two guest programs:

1. `balance_of.rs`: Read balance of an ERC20 token on an EVM chain and generate a zkProof of validity
2. `balance_of_ethereum_light_client.rs`: Read balance of an ERC20 token on Ethereum using the Ethereum Light Client and generate a zkProof of validity

[guest]: https://dev.risczero.com/terminology#guest
[developer docs]: https://dev.risczero.com/zkvm
[guest module]: https://docs.rs/risc0-zkvm/latest/risc0_zkvm/guest/index.html
[`risc0-zkvm`]: https://docs.rs/risc0-zkvm/latest/risc0_zkvm/index.html
