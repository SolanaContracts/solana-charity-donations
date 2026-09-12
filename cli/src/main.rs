use std::rc::Rc;

use anchor_client::{
    solana_sdk::{
        commitment_config::CommitmentConfig,
        pubkey::Pubkey,
        signature::{read_keypair_file, Signer},
    },
    Client, Cluster,
};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use solana_charity_donations::{accounts, instruction, Campaign};
use std::path::PathBuf;

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

fn sol_to_lamports(sol: f64) -> u64 {
    (sol * LAMPORTS_PER_SOL) as u64
}

fn lamports_to_sol(lamports: u64) -> f64 {
    lamports as f64 / LAMPORTS_PER_SOL
}

#[derive(Parser)]
#[command(name = "charity-cli", about = "CLI client for the solana-charity-donations Anchor program")]
struct Cli {
    /// JSON-RPC URL of the cluster to talk to
    #[arg(long, global = true, default_value = "http://127.0.0.1:8899")]
    url: String,

    /// WebSocket URL of the cluster (used for transaction confirmation)
    #[arg(long, global = true, default_value = "ws://127.0.0.1:8900")]
    ws_url: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a new campaign
    InitCampaign {
        /// Keypair file for the campaign authority (also pays for the account)
        #[arg(long)]
        keypair: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: String,
        /// Fundraising goal in SOL (informational only, not enforced on-chain)
        #[arg(long)]
        goal_sol: f64,
        /// Unix timestamp after which donations are rejected; 0 means no deadline
        #[arg(long, default_value_t = 0)]
        deadline_unix: i64,
    },
    /// Donate SOL to a campaign
    Donate {
        /// Keypair file for the donor
        #[arg(long)]
        keypair: PathBuf,
        /// Campaign authority's public key
        #[arg(long)]
        authority: Pubkey,
        #[arg(long)]
        name: String,
        #[arg(long)]
        amount_sol: f64,
    },
    /// Withdraw raised funds (authority only)
    Withdraw {
        /// Keypair file for the campaign authority
        #[arg(long)]
        keypair: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long)]
        amount_sol: f64,
    },
    /// Reclaim your outstanding (non-withdrawn) donation
    Refund {
        /// Keypair file for the donor
        #[arg(long)]
        keypair: PathBuf,
        /// Campaign authority's public key
        #[arg(long)]
        authority: Pubkey,
        #[arg(long)]
        name: String,
    },
    /// Close a campaign once all funds are withdrawn or refunded (authority only)
    Close {
        /// Keypair file for the campaign authority
        #[arg(long)]
        keypair: PathBuf,
        #[arg(long)]
        name: String,
    },
    /// Print a campaign's current on-chain state
    Show {
        /// Campaign authority's public key
        #[arg(long)]
        authority: Pubkey,
        #[arg(long)]
        name: String,
    },
}

fn campaign_pda(authority: &Pubkey, name: &str) -> Pubkey {
    Pubkey::find_program_address(
        &[b"campaign", authority.as_ref(), name.as_bytes()],
        &solana_charity_donations::ID,
    )
    .0
}

fn donation_record_pda(campaign: &Pubkey, donor: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[b"donation", campaign.as_ref(), donor.as_ref()],
        &solana_charity_donations::ID,
    )
    .0
}

fn load_keypair(path: &PathBuf) -> Result<anchor_client::solana_sdk::signature::Keypair> {
    read_keypair_file(path)
        .map_err(|e| anyhow::anyhow!("failed to read keypair at {}: {e}", path.display()))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cluster = Cluster::Custom(cli.url.clone(), cli.ws_url.clone());

    match cli.command {
        Command::InitCampaign {
            keypair,
            name,
            description,
            goal_sol,
            deadline_unix,
        } => {
            let authority = Rc::new(load_keypair(&keypair)?);
            let client = Client::new_with_options(cluster, authority.clone(), CommitmentConfig::confirmed());
            let program = client.program(solana_charity_donations::ID)?;
            let campaign = campaign_pda(&authority.pubkey(), &name);

            let sig = program
                .request()
                .accounts(accounts::InitializeCampaign {
                    authority: authority.pubkey(),
                    campaign,
                    system_program: solana_system_interface::program::ID,
                })
                .args(instruction::InitializeCampaign {
                    name: name.clone(),
                    description,
                    goal_lamports: sol_to_lamports(goal_sol),
                    deadline_unix,
                })
                .send()
                .context("initialize_campaign transaction failed")?;

            println!("Campaign \"{name}\" created at {campaign}");
            println!("Signature: {sig}");
        }

        Command::Donate {
            keypair,
            authority,
            name,
            amount_sol,
        } => {
            let donor = Rc::new(load_keypair(&keypair)?);
            let client = Client::new_with_options(cluster, donor.clone(), CommitmentConfig::confirmed());
            let program = client.program(solana_charity_donations::ID)?;
            let campaign = campaign_pda(&authority, &name);
            let donation_record = donation_record_pda(&campaign, &donor.pubkey());

            let sig = program
                .request()
                .accounts(accounts::Donate {
                    donor: donor.pubkey(),
                    campaign,
                    donation_record,
                    system_program: solana_system_interface::program::ID,
                })
                .args(instruction::Donate {
                    amount: sol_to_lamports(amount_sol),
                })
                .send()
                .context("donate transaction failed")?;

            println!("Donated {amount_sol} SOL to \"{name}\"");
            println!("Signature: {sig}");
        }

        Command::Withdraw {
            keypair,
            name,
            amount_sol,
        } => {
            let authority = Rc::new(load_keypair(&keypair)?);
            let client = Client::new_with_options(cluster, authority.clone(), CommitmentConfig::confirmed());
            let program = client.program(solana_charity_donations::ID)?;
            let campaign = campaign_pda(&authority.pubkey(), &name);

            let sig = program
                .request()
                .accounts(accounts::Withdraw {
                    authority: authority.pubkey(),
                    campaign,
                })
                .args(instruction::Withdraw {
                    amount: sol_to_lamports(amount_sol),
                })
                .send()
                .context("withdraw transaction failed")?;

            println!("Withdrew {amount_sol} SOL from \"{name}\"");
            println!("Signature: {sig}");
        }

        Command::Refund {
            keypair,
            authority,
            name,
        } => {
            let donor = Rc::new(load_keypair(&keypair)?);
            let client = Client::new_with_options(cluster, donor.clone(), CommitmentConfig::confirmed());
            let program = client.program(solana_charity_donations::ID)?;
            let campaign = campaign_pda(&authority, &name);
            let donation_record = donation_record_pda(&campaign, &donor.pubkey());

            let sig = program
                .request()
                .accounts(accounts::RequestRefund {
                    donor: donor.pubkey(),
                    campaign,
                    donation_record,
                })
                .args(instruction::RequestRefund {})
                .send()
                .context("request_refund transaction failed")?;

            println!("Refunded outstanding donation from \"{name}\"");
            println!("Signature: {sig}");
        }

        Command::Close { keypair, name } => {
            let authority = Rc::new(load_keypair(&keypair)?);
            let client = Client::new_with_options(cluster, authority.clone(), CommitmentConfig::confirmed());
            let program = client.program(solana_charity_donations::ID)?;
            let campaign = campaign_pda(&authority.pubkey(), &name);

            let sig = program
                .request()
                .accounts(accounts::CloseCampaign {
                    authority: authority.pubkey(),
                    campaign,
                })
                .args(instruction::CloseCampaign {})
                .send()
                .context("close_campaign transaction failed")?;

            println!("Closed campaign \"{name}\"");
            println!("Signature: {sig}");
        }

        Command::Show { authority, name } => {
            // Show doesn't sign anything, but anchor_client still requires a payer keypair
            // to construct a Program handle; a throwaway keypair is fine for read-only calls.
            let dummy_payer = Rc::new(anchor_client::solana_sdk::signature::Keypair::new());
            let client = Client::new_with_options(cluster, dummy_payer, CommitmentConfig::confirmed());
            let program = client.program(solana_charity_donations::ID)?;
            let campaign_key = campaign_pda(&authority, &name);
            let campaign: Campaign = program
                .account(campaign_key)
                .context("failed to fetch campaign account (does it exist?)")?;

            println!("Campaign: {}", campaign.name);
            println!("  address:          {campaign_key}");
            println!("  authority:        {}", campaign.authority);
            println!("  description:      {}", campaign.description);
            println!(
                "  goal:             {} SOL",
                lamports_to_sol(campaign.goal_lamports)
            );
            println!("  deadline (unix):  {}", campaign.deadline_unix);
            println!(
                "  raised:           {} SOL",
                lamports_to_sol(campaign.amount_raised)
            );
            println!(
                "  withdrawn:        {} SOL",
                lamports_to_sol(campaign.amount_withdrawn)
            );
            println!("  donor count:      {}", campaign.donor_count);
        }
    }

    Ok(())
}
