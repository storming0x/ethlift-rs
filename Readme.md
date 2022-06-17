## ETHLIFT

**ETHLIFT is a set of CLI tools intended to be use by smart contract developers for general tasks not covered at the moment by other CLI tools**

## Installation

TODO: how to install from release binary

### Installing from Source

For people that want to install from source, you can do so like below:

```sh
git clone https://github.com/storming0x/ethlift-rs
cd ethlift-rs
cargo install --path ./ --bins --locked --force
```

Or via `cargo install --git https://github.com/storming0x/ethlift-rs --locked ethlift`.

### Manual Download

TODO:

You can manually download nightly releases [here](https://github.com/storming0x/ethlift-rs/releases).

## ethlift

### Commands

- **ethdiff**
  - Get a diff of a local solidity file compared against the deployed etherescan verified source code
  - Supports Brownie and foundry projects. TODO: hardhat
- **TODO flatten** 
  - Generic flattener for solidity contracts to merge imports into a single file
  - Supports Brownie and foundry projects

## Contributing

See our [contributing guidelines](./CONTRIBUTING.md).

## Getting Help

- Open an issue with [the bug](https://github.com/storming0x/ethlift-rs/issues/new)

## Acknowledgements

- Foundry and ethers-rs contributors.
