# solana-charity-donations

An Anchor program for transparent, on-chain charity/crowdfunding campaigns. Anyone can donate SOL to a campaign, the campaign owner can withdraw funds as needed, and donors can reclaim any part of their donation the owner hasn't withdrawn yet.

## Why

A donation platform's value over "just send SOL to a wallet" is transparency and donor protection:

- Anyone can see how much a campaign has raised and how much the owner has withdrawn — it's all on-chain.
- Donors get a `request_refund` escape hatch for funds the owner hasn't withdrawn yet, instead of blind trust.
- The owner has flexible access to funds (no all-or-nothing goal gating), which matters for real charity use cases like disaster relief where money is needed immediately.

## Instructions

| Instruction | Signer | Description |
|---|---|---|
| `initialize_campaign(name, description, goal_lamports, deadline_unix)` | authority | Creates a `Campaign` PDA. `deadline_unix = 0` means no deadline. |
| `donate(amount)` | donor | Transfers `amount` lamports from the donor to the campaign PDA. Tracks the donor's contribution in a `DonationRecord` PDA. Fails if the deadline has passed. |
| `withdraw(amount)` | authority | Moves up to the campaign's withdrawable balance (raised minus already withdrawn, above rent-exempt minimum) to the authority. |
| `request_refund()` | donor | Refunds the donor's full outstanding (non-refunded, non-withdrawn) contribution. Fails naturally if the owner already withdrew those funds. |
| `close_campaign()` | authority | Closes the `Campaign` account and reclaims rent. Only allowed once `amount_raised == amount_withdrawn` (no funds owed to donors). |

## Accounts

**`Campaign`** — PDA at `["campaign", authority, name]`
- `authority: Pubkey`, `name: String`, `description: String`
- `goal_lamports: u64` (informational, not enforced), `deadline_unix: i64`
- `amount_raised: u64`, `amount_withdrawn: u64`, `donor_count: u64`, `bump: u8`

**`DonationRecord`** — PDA at `["donation", campaign, donor]`
- `donor: Pubkey`, `campaign: Pubkey`, `amount: u64` (outstanding, non-refunded contribution), `bump: u8`

## Building and testing

Requires `solana-cli`, `anchor-cli`, and Rust already installed.

This machine's default Solana `platform-tools` (v1.48, bundled `rustc` 1.84) can't compile some current crates.io dependency versions that require the `edition2024` Cargo feature. Fixed here by force-installing `platform-tools` v1.57 and pinning `proc-macro-crate` down in `Cargo.lock`. If you hit `edition2024` build errors on a fresh machine:

```bash
cargo-build-sbf --force-tools-install --tools-version v1.57
```

Then build, generate the IDL, and test with the pinned tools version:

```bash
anchor build --no-idl -- --tools-version v1.57
anchor idl build -o target/idl/solana_charity_donations.json -t target/types/solana_charity_donations.ts
anchor test --skip-build --no-idl
```

`cargo clippy` (run from `programs/solana-charity-donations`) is clean.

## CLI client

`cli/` is a Rust CLI (`charity-cli`, built with `anchor-client` + `clap`) for calling every instruction from the terminal, plus a `show` command to read a campaign's on-chain state. It talks to `http://127.0.0.1:8899` / `ws://127.0.0.1:8900` (a local validator) by default — override with `--url` / `--ws-url` for devnet or mainnet.

```bash
cargo build -p charity-cli
BIN=./target/debug/charity-cli

# start a local validator and deploy first:
solana-test-validator --reset --quiet &
solana program deploy target/deploy/solana_charity_donations.so \
  --program-id target/deploy/solana_charity_donations-keypair.json

$BIN init-campaign --keypair ~/authority.json --name flood-relief \
  --description "Emergency flood relief" --goal-sol 10 --deadline-unix 0

$BIN donate --keypair ~/donor.json --authority <AUTHORITY_PUBKEY> \
  --name flood-relief --amount-sol 1.5

$BIN show --authority <AUTHORITY_PUBKEY> --name flood-relief

$BIN withdraw --keypair ~/authority.json --name flood-relief --amount-sol 0.5
$BIN refund --keypair ~/donor.json --authority <AUTHORITY_PUBKEY> --name flood-relief
$BIN close --keypair ~/authority.json --name flood-relief
```

Run `$BIN --help` or `$BIN <command> --help` for the full flag list.

## Known limitation

`target/` is gitignored, so the program's keypair (and its on-chain program ID) regenerates on a fresh clone/build. Not an issue pre-deployment, but before deploying to devnet/mainnet you'll want to commit or otherwise persist `target/deploy/solana_charity_donations-keypair.json` so the program ID stays stable across builds.
